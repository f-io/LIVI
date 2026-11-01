use std::{collections::HashMap, mem, rc::Rc};

#[cfg(not(target_arch = "wasm32"))]
use std::ffi::c_void;

#[cfg(all(feature = "glutin", not(target_arch = "wasm32")))]
use glutin::display::GlDisplay;

use fnv::FnvHashMap;
use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    renderer::{GlyphTexture, ImageId, Vertex},
    BlendFactor, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageInfo, ImageSource, ImageStore,
    Scissor, Transform2D,
};

use glow::HasContext;

use super::{Command, CommandType, Params, RenderTarget, Renderer, ShaderType, SurfacelessRenderer};

mod program;
use program::MainProgram;

mod gl_texture;
use gl_texture::GlTexture;

mod framebuffer;
use framebuffer::Framebuffer;

mod uniform_array;
use uniform_array::UniformArray;

/// Represents an OpenGL renderer.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct OpenGl {
    debug: bool,
    antialias: bool,
    is_opengles_2_0: bool,
    view: [f32; 2],
    screen_view: [f32; 2],
    // All types of the vertex/fragment shader, indexed by shader_type when has_glyph_texture is true
    main_programs_with_glyph_texture: [Option<MainProgram>; 11],
    // Same shader programs but with has_glyph_texture being false
    main_programs_without_glyph_texture: [Option<MainProgram>; 11],
    current_program: u8,
    current_program_needs_glyph_texture: bool,
    vert_arr: Option<<glow::Context as glow::HasContext>::VertexArray>,
    vert_buff: Option<<glow::Context as glow::HasContext>::Buffer>,
    framebuffers: FnvHashMap<ImageId, Result<Framebuffer, ErrorKind>>,
    context: Rc<glow::Context>,
    screen_target: Option<Framebuffer>,
    current_render_target: RenderTarget,
}

impl OpenGl {
    /// Creates a new OpenGL renderer from a function loader.
    ///
    /// # Safety
    /// This function is unsafe because it requires a function loader that can load OpenGL functions
    /// and create a valid OpenGL context.
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn new_from_function<F>(load_fn: F) -> Result<Self, ErrorKind>
    where
        F: FnMut(&str) -> *const c_void,
    {
        let context = glow::Context::from_loader_function(load_fn);
        let version = context.get_parameter_string(glow::VERSION);
        let is_opengles_2_0 = version.starts_with("OpenGL ES 2.");
        Self::new_from_context(context, is_opengles_2_0)
    }

    /// Creates a new OpenGL renderer from a function loader that takes C-style strings.
    ///
    /// # Safety
    /// This function is unsafe because it requires a function loader that can load OpenGL functions
    /// and create a valid OpenGL context.
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn new_from_function_cstr<F>(load_fn: F) -> Result<Self, ErrorKind>
    where
        F: FnMut(&std::ffi::CStr) -> *const c_void,
    {
        let context = glow::Context::from_loader_function_cstr(load_fn);
        let version = context.get_parameter_string(glow::VERSION);
        let is_opengles_2_0 = version.starts_with("OpenGL ES 2.");
        Self::new_from_context(context, is_opengles_2_0)
    }

    /// Creates a new OpenGL renderer from a Glutin display.
    #[cfg(all(feature = "glutin", not(target_arch = "wasm32")))]
    pub fn new_from_glutin_display(display: &impl GlDisplay) -> Result<Self, ErrorKind> {
        unsafe { OpenGl::new_from_function_cstr(|s| display.get_proc_address(s).cast()) }
    }

    /// Creates a new OpenGL renderer from an HTML canvas element in a WASM32 target.
    #[cfg(target_arch = "wasm32")]
    pub fn new_from_html_canvas(canvas: &web_sys::HtmlCanvasElement) -> Result<Self, ErrorKind> {
        let attrs = web_sys::WebGlContextAttributes::new();
        attrs.set_stencil(true);
        attrs.set_antialias(false);

        use wasm_bindgen::JsCast;
        let webgl2_context = match canvas.get_context_with_context_options("webgl2", &attrs) {
            Ok(Some(context)) => context.dyn_into::<web_sys::WebGl2RenderingContext>().unwrap(),
            _ => {
                return Err(ErrorKind::GeneralError(
                    "Canvas::getContext failed to retrieve WebGL 2 context".to_owned(),
                ))
            }
        };

        let context = glow::Context::from_webgl2_context(webgl2_context);
        Self::new_from_context(context, true)
    }

    fn new_from_context(context: glow::Context, is_opengles_2_0: bool) -> Result<Self, ErrorKind> {
        let debug = cfg!(debug_assertions);
        let antialias = true;

        let context = Rc::new(context);

        let generate_shader_program_variants = |with_glyph_texture| -> Result<_, ErrorKind> {
            Ok([
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillGradient,
                    with_glyph_texture,
                )?),
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillImage,
                    with_glyph_texture,
                )?),
                if with_glyph_texture {
                    // No stencil fill with glyph texture
                    None
                } else {
                    Some(MainProgram::new(&context, antialias, ShaderType::Stencil, false)?)
                },
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillImageGradient,
                    with_glyph_texture,
                )?),
                if with_glyph_texture {
                    // Image filter is unrelated to glyph rendering
                    None
                } else {
                    Some(MainProgram::new(&context, antialias, ShaderType::FilterImage, false)?)
                },
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillColor,
                    with_glyph_texture,
                )?),
                if with_glyph_texture {
                    // Texture blitting is unrelated to glyph rendering
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::TextureCopyUnclipped,
                        false,
                    )?)
                },
                None,
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillGradientConic,
                    with_glyph_texture,
                )?),
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillImageGradientConic,
                    with_glyph_texture,
                )?),
                if with_glyph_texture {
                    // Image filter is unrelated to glyph rendering
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageColorMatrix,
                        false,
                    )?)
                },
            ])
        };

        let main_programs_with_glyph_texture = generate_shader_program_variants(true)?;
        let main_programs_without_glyph_texture = generate_shader_program_variants(false)?;

        let mut opengl = Self {
            debug,
            antialias,
            is_opengles_2_0: false,
            view: [0.0, 0.0],
            screen_view: [0.0, 0.0],
            main_programs_with_glyph_texture,
            main_programs_without_glyph_texture,
            current_program: 0,
            current_program_needs_glyph_texture: true,
            vert_arr: None,
            vert_buff: None,
            framebuffers: HashMap::default(),
            context,
            screen_target: None,
            current_render_target: RenderTarget::Screen,
        };

        unsafe {
            opengl.is_opengles_2_0 = is_opengles_2_0;

            opengl.vert_arr = opengl.context.create_vertex_array().ok();
            opengl.vert_buff = opengl.context.create_buffer().ok();
        }

        Ok(opengl)
    }

    /// Checks if the renderer is using OpenGL ES.
    pub fn is_opengles(&self) -> bool {
        self.is_opengles_2_0
    }

    fn check_error(&self, label: &str) {
        if !self.debug {
            return;
        }

        let err = unsafe { self.context.get_error() };

        if err == glow::NO_ERROR {
            return;
        }

        let message = match err {
            glow::INVALID_ENUM => "Invalid enum",
            glow::INVALID_VALUE => "Invalid value",
            glow::INVALID_OPERATION => "Invalid operation",
            glow::OUT_OF_MEMORY => "Out of memory",
            glow::INVALID_FRAMEBUFFER_OPERATION => "Invalid framebuffer operation",
            _ => "Unknown error",
        };

        log::error!("({err}) Error on {label} - {message}");
    }

    fn gl_factor(factor: BlendFactor) -> u32 {
        match factor {
            BlendFactor::Zero => glow::ZERO,
            BlendFactor::One => glow::ONE,
            BlendFactor::SrcColor => glow::SRC_COLOR,
            BlendFactor::OneMinusSrcColor => glow::ONE_MINUS_SRC_COLOR,
            BlendFactor::DstColor => glow::DST_COLOR,
            BlendFactor::OneMinusDstColor => glow::ONE_MINUS_DST_COLOR,
            BlendFactor::SrcAlpha => glow::SRC_ALPHA,
            BlendFactor::OneMinusSrcAlpha => glow::ONE_MINUS_SRC_ALPHA,
            BlendFactor::DstAlpha => glow::DST_ALPHA,
            BlendFactor::OneMinusDstAlpha => glow::ONE_MINUS_DST_ALPHA,
            BlendFactor::SrcAlphaSaturate => glow::SRC_ALPHA_SATURATE,
        }
    }

    fn set_composite_operation(&self, blend_state: CompositeOperationState) {
        unsafe {
            self.context.blend_func_separate(
                Self::gl_factor(blend_state.src_rgb),
                Self::gl_factor(blend_state.dst_rgb),
                Self::gl_factor(blend_state.src_alpha),
                Self::gl_factor(blend_state.dst_alpha),
            );
        }
    }

    fn convex_fill(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, gpu_paint: &Params) {
        self.set_uniforms(images, gpu_paint, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe {
                    self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
                }
            }

            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.check_error("convex_fill");
    }

    fn concave_fill(
        &mut self,
        images: &ImageStore<GlTexture>,
        cmd: &Command,
        stencil_paint: &Params,
        fill_paint: &Params,
    ) {
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.stencil_mask(0xff);
            self.context.stencil_func(glow::ALWAYS, 0, 0xff);
            self.context.color_mask(false, false, false, false);
            //glow::DepthMask(glow::FALSE);
        }

        self.set_uniforms(images, stencil_paint, None, GlyphTexture::None);

        unsafe {
            self.context
                .stencil_op_separate(glow::FRONT, glow::KEEP, glow::KEEP, glow::INCR_WRAP);
            self.context
                .stencil_op_separate(glow::BACK, glow::KEEP, glow::KEEP, glow::DECR_WRAP);
            self.context.disable(glow::CULL_FACE);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe {
                    self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
                }
            }
        }

        unsafe {
            self.context.enable(glow::CULL_FACE);
            // Draw anti-aliased pixels
            self.context.color_mask(true, true, true, true);
            //glow::DepthMask(glow::TRUE);
        }

        self.set_uniforms(images, fill_paint, cmd.image, cmd.glyph_texture);

        if self.antialias {
            unsafe {
                match cmd.fill_rule {
                    FillRule::NonZero => self.context.stencil_func(glow::EQUAL, 0x0, 0xff),
                    FillRule::EvenOdd => self.context.stencil_func(glow::EQUAL, 0x0, 0x1),
                }

                self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
            }

            // draw fringes
            for drawable in &cmd.drawables {
                if let Some((start, count)) = drawable.stroke_verts {
                    unsafe {
                        self.context
                            .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                    }
                }
            }
        }

        unsafe {
            match cmd.fill_rule {
                FillRule::NonZero => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0xff),
                FillRule::EvenOdd => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0x1),
            }

            self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);

            if let Some((start, count)) = cmd.triangles_verts {
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
            }

            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("concave_fill");
    }

    fn stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.check_error("stroke");
    }

    fn stencil_stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint1: &Params, paint2: &Params) {
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.stencil_mask(0xff);

            // Fill the stroke base without overlap
            self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::INCR);
        }

        self.set_uniforms(images, paint2, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        // Draw anti-aliased pixels.
        self.set_uniforms(images, paint1, cmd.image, cmd.glyph_texture);

        unsafe {
            self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        unsafe {
            // Clear stencil buffer.
            self.context.color_mask(false, false, false, false);
            self.context.stencil_func(glow::ALWAYS, 0x0, 0xff);
            self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        unsafe {
            self.context.color_mask(true, true, true, true);
            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("stencil_stroke");
    }

    fn triangles(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe {
                self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
            }
        }

        self.check_error("triangles");
    }

    fn set_uniforms(
        &mut self,
        images: &ImageStore<GlTexture>,
        paint: &Params,
        image_tex: Option<ImageId>,
        glyph_tex: GlyphTexture,
    ) {
        self.select_main_program(paint);
        let arr = UniformArray::from(paint);
        self.main_program().set_config(arr.as_slice());
        self.check_error("set_uniforms uniforms");

        let tex = image_tex.and_then(|id| images.get(id)).map(GlTexture::id);
        let is_external = image_tex
            .and_then(|id| images.get(id))
            .map(|t| t.external)
            .unwrap_or(false);

        const TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

        unsafe {
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(
                if is_external {
                    TEXTURE_EXTERNAL_OES
                } else {
                    glow::TEXTURE_2D
                },
                tex,
            );
        }

        let glyphtex = glyph_tex.image_id().and_then(|id| images.get(id).map(GlTexture::id));

        unsafe {
            self.context.active_texture(glow::TEXTURE0 + 1);
            self.context.bind_texture(glow::TEXTURE_2D, glyphtex);
        }

        self.check_error("set_uniforms texture");
    }

    fn clear_rect(&self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        unsafe {
            self.context.enable(glow::SCISSOR_TEST);
            self.context.scissor(
                x as i32,
                self.view[1] as i32 - (height as i32 + y as i32),
                width as i32,
                height as i32,
            );
            self.context.clear_color(color.r, color.g, color.b, color.a);
            self.context.clear(glow::COLOR_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            self.context.disable(glow::SCISSOR_TEST);
        }
    }

    fn set_target(&mut self, images: &ImageStore<GlTexture>, target: RenderTarget) {
        self.current_render_target = target;
        match (target, &self.screen_target) {
            (RenderTarget::Screen, None) => unsafe {
                Framebuffer::unbind(&self.context);
                self.view = self.screen_view;
                self.context.viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
            },
            (RenderTarget::Screen, Some(framebuffer)) => {
                framebuffer.bind();
                self.view = self.screen_view;
                unsafe {
                    self.context.viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
                }
            }
            (RenderTarget::Image(id), _) => {
                let context = self.context.clone();
                if let Some(texture) = images.get(id) {
                    if let Ok(fb) = self
                        .framebuffers
                        .entry(id)
                        .or_insert_with(|| Framebuffer::new(&context, texture))
                    {
                        fb.bind();

                        self.view[0] = texture.info().width() as f32;
                        self.view[1] = texture.info().height() as f32;

                        unsafe {
                            self.context
                                .viewport(0, 0, texture.info().width() as i32, texture.info().height() as i32);
                        }
                    }
                }
            }
        }
    }

    /// Make the "Screen" `RenderTarget` actually render to a framebuffer object. This is useful when
    /// embedding femtovg into another program where final composition is handled by an external task.
    /// The given `framebuffer_object` must refer to a Framebuffer Object created on the current OpenGL
    /// Context, and must have a depth & stencil attachment.
    ///
    /// Pass `None` to clear any previous Framebuffer Object ID that was passed and target rendering to
    /// the default target (normally the window).
    pub fn set_screen_target(&mut self, framebuffer_object: Option<<glow::Context as glow::HasContext>::Framebuffer>) {
        match framebuffer_object {
            Some(fbo_id) => self.screen_target = Some(Framebuffer::from_external(&self.context, fbo_id)),
            None => self.screen_target = None,
        }
    }

    fn render_filtered_image(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        filter: ImageFilter,
    ) {
        match filter {
            ImageFilter::GaussianBlur { sigma } => self.render_gaussian_blur(images, cmd, target_image, sigma),
            ImageFilter::ColorMatrix { matrix } => self.render_color_matrix(images, cmd, target_image, matrix),
        }
    }

    fn render_color_matrix(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        matrix: [f32; 20],
    ) {
        let original_render_target = self.current_render_target;
        let source_image_info = images.get(cmd.image.unwrap()).unwrap().info();

        let image_paint = crate::Paint::image(
            cmd.image.unwrap(),
            0.,
            0.,
            source_image_info.width() as _,
            source_image_info.height() as _,
            0.,
            1.,
        );
        let mut params = Params::new(
            images,
            &Transform2D::default(),
            &image_paint.flavor,
            &GlyphTexture::default(),
            &Scissor::default(),
            0.,
            0.,
            0.,
        );
        params.shader_type = ShaderType::FilterImageColorMatrix;
        // The color matrix rides the scissor/paint-matrix uniform slots, which are
        // dead during a filter pass (no scissor, no paint gradient): frag[0..2]
        // hold the first 12 values, frag[3..4] the last 8, so no uniform-array
        // growth is needed. The shader reads them back as a row-major 4x5 matrix.
        params.scissor_mat.copy_from_slice(&matrix[..12]);
        params.paint_mat[..8].copy_from_slice(&matrix[12..20]);

        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program().set_view(self.view);
        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
        );
        self.triangles(images, &cmd, &params);

        self.set_target(images, original_render_target);
        self.main_program().set_view(self.view);
    }

    fn render_gaussian_blur(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        mut cmd: Command,
        target_image: ImageId,
        sigma: f32,
    ) {
        let original_render_target = self.current_render_target;

        // The filtering happens in two passes, first a horizontal blur and then the vertial blur. The
        // first pass therefore renders into an intermediate, temporarily allocated texture.

        let source_image_info = images.get(cmd.image.unwrap()).unwrap().info();

        let image_paint = crate::Paint::image(
            cmd.image.unwrap(),
            0.,
            0.,
            source_image_info.width() as _,
            source_image_info.height() as _,
            0.,
            1.,
        );
        let mut blur_params = Params::new(
            images,
            &Transform2D::default(),
            &image_paint.flavor,
            &GlyphTexture::default(),
            &Scissor::default(),
            0.,
            0.,
            0.,
        );
        blur_params.shader_type = ShaderType::FilterImage;

        let gauss_coeff_x = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
        let gauss_coeff_y = f32::exp(-0.5 / (sigma * sigma));
        let gauss_coeff_z = gauss_coeff_y * gauss_coeff_y;

        blur_params.image_blur_filter_coeff[0] = gauss_coeff_x;
        blur_params.image_blur_filter_coeff[1] = gauss_coeff_y;
        blur_params.image_blur_filter_coeff[2] = gauss_coeff_z;

        blur_params.image_blur_filter_direction = [1.0, 0.0];

        // GLES 2.0 does not allow non-constant loop indices, so limit the standard devitation to allow for a upper fixed limit
        // on the number of iterations in the fragment shader.
        blur_params.image_blur_filter_sigma = sigma.min(8.);

        let horizontal_blur_buffer = images.alloc(self, source_image_info).unwrap();
        self.set_target(images, RenderTarget::Image(horizontal_blur_buffer));
        self.main_program().set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
        );

        self.triangles(images, &cmd, &blur_params);

        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program().set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
        );

        blur_params.image_blur_filter_direction = [0.0, 1.0];

        cmd.image = Some(horizontal_blur_buffer);

        self.triangles(images, &cmd, &blur_params);

        images.remove(self, horizontal_blur_buffer);

        // restore previous render target and view
        self.set_target(images, original_render_target);
        self.main_program().set_view(self.view);
    }

    fn main_program(&self) -> &MainProgram {
        let programs = if self.current_program_needs_glyph_texture {
            &self.main_programs_with_glyph_texture
        } else {
            &self.main_programs_without_glyph_texture
        };
        programs[self.current_program as usize]
            .as_ref()
            .expect("internal error: invalid shader program selected for given paint")
    }

    fn select_main_program(&mut self, params: &Params) {
        let program_index = params.shader_type.to_u8();
        if program_index != self.current_program
            || params.uses_glyph_texture() != self.current_program_needs_glyph_texture
        {
            unsafe {
                self.context.active_texture(glow::TEXTURE0);
                self.context.bind_texture(glow::TEXTURE_2D, None);
                self.context.active_texture(glow::TEXTURE0 + 1);
                self.context.bind_texture(glow::TEXTURE_2D, None);
            }

            self.main_program().unbind();
            self.current_program = program_index;
            self.current_program_needs_glyph_texture = params.uses_glyph_texture();

            let program = self.main_program();
            program.bind();
            // Bind the two uniform samplers to texture units
            program.set_tex(0);
            program.set_glyphtex(1);
            program.set_view(self.view);
        }
    }
}

impl Renderer for OpenGl {
    type Image = GlTexture;
    type NativeTexture = <glow::Context as glow::HasContext>::Texture;
    type ExternalTexture = <glow::Context as glow::HasContext>::Texture;
    type RenderOutput = ();
    type CommandBuffer = ();

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;

        self.screen_view = self.view;

        unsafe {
            self.context.viewport(0, 0, width as i32, height as i32);
        }
    }

    fn get_native_texture(&self, image: &Self::Image) -> Result<Self::NativeTexture, ErrorKind> {
        Ok(image.id())
    }

    fn render(
        &mut self,
        _output: impl Into<Self::RenderOutput>,
        images: &mut ImageStore<Self::Image>,
        verts: &[Vertex],
        commands: Vec<Command>,
    ) {
        self.current_program = 0;
        self.main_program().bind();

        unsafe {
            self.context.enable(glow::CULL_FACE);

            self.context.cull_face(glow::BACK);
            self.context.front_face(glow::CCW);
            self.context.enable(glow::BLEND);
            self.context.disable(glow::DEPTH_TEST);
            self.context.disable(glow::SCISSOR_TEST);
            self.context.color_mask(true, true, true, true);
            self.context.stencil_mask(0xffff_ffff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
            self.context.stencil_func(glow::ALWAYS, 0, 0xffff_ffff);
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(glow::TEXTURE_2D, None);
            self.context.active_texture(glow::TEXTURE0 + 1);
            self.context.bind_texture(glow::TEXTURE_2D, None);

            self.context.bind_vertex_array(self.vert_arr);

            let vertex_size = mem::size_of::<Vertex>();

            self.context.bind_buffer(glow::ARRAY_BUFFER, self.vert_buff);
            self.context
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, verts.align_to().1, glow::STREAM_DRAW);

            self.context.enable_vertex_attrib_array(0);
            self.context.enable_vertex_attrib_array(1);

            self.context
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, vertex_size as i32, 0);
            self.context.vertex_attrib_pointer_f32(
                1,
                2,
                glow::FLOAT,
                false,
                vertex_size as i32,
                2 * mem::size_of::<f32>() as i32,
            );
        }

        self.check_error("render prepare");

        for cmd in commands {
            self.set_composite_operation(cmd.composite_operation);

            match cmd.cmd_type {
                CommandType::ConvexFill { ref params } => self.convex_fill(images, &cmd, params),
                CommandType::ConcaveFill {
                    ref stencil_params,
                    ref fill_params,
                } => self.concave_fill(images, &cmd, stencil_params, fill_params),
                CommandType::Stroke { ref params } => self.stroke(images, &cmd, params),
                CommandType::StencilStroke {
                    ref params1,
                    ref params2,
                } => self.stencil_stroke(images, &cmd, params1, params2),
                CommandType::Triangles { ref params } => self.triangles(images, &cmd, params),
                CommandType::ClearRect { color } => {
                    if let Some((start, _)) = cmd.triangles_verts {
                        let x = verts[start].x as _;
                        let y = verts[start].y as _;
                        let width = verts[start + 1].x as u32 - x;
                        let height = verts[start + 1].y as u32 - y;
                        self.clear_rect(x, y, width, height, color);
                    }
                }
                CommandType::SetRenderTarget(target) => {
                    self.set_target(images, target);
                    self.main_program().set_view(self.view);
                }
                CommandType::RenderFilteredImage { target_image, filter } => {
                    self.render_filtered_image(images, cmd, target_image, filter)
                }
            }
        }

        unsafe {
            self.context.disable_vertex_attrib_array(0);
            self.context.disable_vertex_attrib_array(1);
            self.context.bind_vertex_array(None);

            self.context.disable(glow::CULL_FACE);
            self.context.bind_buffer(glow::ARRAY_BUFFER, None);
            self.context.bind_texture(glow::TEXTURE_2D, None);
        }

        self.main_program().unbind();

        self.check_error("render done");
    }

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
        Self::Image::new(&self.context, info, self.is_opengles_2_0)
    }

    fn create_image_from_native_texture(
        &mut self,
        native_texture: Self::NativeTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind> {
        Ok(Self::Image::new_from_native_texture(native_texture, info, false))
    }

    fn create_image_from_external_texture(
        &mut self,
        native_texture: Self::ExternalTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind> {
        Ok(Self::Image::new_from_native_texture(native_texture, info, true))
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        image.update(&self.context, data, x, y, self.is_opengles_2_0)
    }

    fn delete_image(&mut self, image: Self::Image, image_id: ImageId) {
        self.framebuffers.remove(&image_id);
        image.delete(&self.context);
    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        //let mut image = image::RgbaImage::new(self.view[0] as u32, self.view[1] as u32);
        let w = self.view[0] as usize;
        let h = self.view[1] as usize;

        let mut image = ImgVec::new(
            vec![
                RGBA8 {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255
                };
                w * h
            ],
            w,
            h,
        );

        unsafe {
            self.context.read_pixels(
                0,
                0,
                self.view[0] as i32,
                self.view[1] as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(image.buf_mut().align_to_mut().1)),
            );
        }

        let mut flipped = Vec::with_capacity(w * h);

        for row in image.rows().rev() {
            flipped.extend_from_slice(row);
        }

        Ok(ImgVec::new(flipped, w, h))
    }
}

impl SurfacelessRenderer for OpenGl {
    fn render_surfaceless(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>) {
        self.render((), images, verts, commands)
    }
}

impl Drop for OpenGl {
    fn drop(&mut self) {
        if let Some(vert_arr) = self.vert_arr {
            unsafe {
                self.context.delete_vertex_array(vert_arr);
            }
        }

        if let Some(vert_buff) = self.vert_buff {
            unsafe {
                self.context.delete_buffer(vert_buff);
            }
        }
    }
}
