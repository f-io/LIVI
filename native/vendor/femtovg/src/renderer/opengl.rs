use std::{collections::HashMap, mem, rc::Rc};

#[cfg(not(wasm_unknown))]
use std::ffi::c_void;

#[cfg(all(feature = "glutin", not(target_arch = "wasm32")))]
use glutin::display::GlDisplay;

use fnv::FnvHashMap;
use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    renderer::{GlyphTexture, ImageId, Vertex},
    BlendFactor, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageFlags, ImageInfo, ImageSource,
    ImageStore, PixelFormat, Scissor, Transform2D,
};

use glow::HasContext;

use super::{Command, CommandType, FanIndices, Params, RenderTarget, Renderer, ShaderType, SurfacelessRenderer};

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
    main_programs_with_glyph_texture: [Option<MainProgram>; 19],
    // Same shader programs but with has_glyph_texture being false
    main_programs_without_glyph_texture: [Option<MainProgram>; 19],
    // The programs of draws under a clip shape, by shader type and glyph texture use, each built when first drawn
    // with (None if that failed): no other program evaluates a shape, and few of them are ever drawn under one.
    clip_programs: FnvHashMap<(u8, bool, (bool, bool)), Option<MainProgram>>,
    // The scissor rect of the draws under a clip; none is no scissor.
    current_clip_bounds: Option<[u32; 4]>,
    current_program: u8,
    current_program_needs_glyph_texture: bool,
    current_program_clips: (bool, bool),
    vert_arr: Option<<glow::Context as glow::HasContext>::VertexArray>,
    vert_buff: Option<<glow::Context as glow::HasContext>::Buffer>,
    // The fill fans as triangles of 32-bit indices, on WebGL alone: its
    // implementations (ANGLE) emulate a fan draw by draw, where a native
    // driver draws one for less than an indexed draw costs (Apple's: by
    // about 7,000 instructions).
    fan_indices: FanIndices,
    fan_index_buff: Option<<glow::Context as glow::HasContext>::Buffer>,
    framebuffers: FnvHashMap<ImageId, Result<Framebuffer, ErrorKind>>,
    context: Rc<glow::Context>,
    screen_target: Option<Framebuffer>,
    current_render_target: RenderTarget,
    max_texture_size: usize,
    // The screen's pixels a blur reads, at their place on the screen:
    // the screen can't be sampled. Kept at the screen's size, since a blur
    // drawn every frame copies every frame.
    screen_copy: Option<GlTexture>,
    // Blurs run every frame, so a skipped one logs once, not per frame.
    warned_blur_skipped: bool,
}

/// The texture parameters a blur's first draw reads an image target
/// with.
const RECT_BLUR_SAMPLING: [(u32, u32); 4] = [
    (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
    (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
    (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
    (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
];

impl OpenGl {
    /// Creates a new OpenGL renderer from a function loader.
    ///
    /// # Safety
    /// This function is unsafe because it requires a function loader that can load OpenGL functions
    /// and create a valid OpenGL context.
    #[cfg(not(wasm_unknown))]
    pub unsafe fn new_from_function<F>(load_fn: F) -> Result<Self, ErrorKind>
    where
        F: FnMut(&str) -> *const c_void,
    {
        let context = glow::Context::from_loader_function(load_fn);
        let version = context.get_parameter_string(glow::VERSION);
        let is_opengles_2_0 = version.starts_with("OpenGL ES 2.");
        Self::new_from_context(context, is_opengles_2_0, false)
    }

    /// Creates a new OpenGL renderer from a function loader that takes C-style strings.
    ///
    /// # Safety
    /// This function is unsafe because it requires a function loader that can load OpenGL functions
    /// and create a valid OpenGL context.
    #[cfg(not(wasm_unknown))]
    pub unsafe fn new_from_function_cstr<F>(load_fn: F) -> Result<Self, ErrorKind>
    where
        F: FnMut(&std::ffi::CStr) -> *const c_void,
    {
        let context = glow::Context::from_loader_function_cstr(load_fn);
        let version = context.get_parameter_string(glow::VERSION);
        let is_opengles_2_0 = version.starts_with("OpenGL ES 2.");
        Self::new_from_context(context, is_opengles_2_0, false)
    }

    /// Creates a new OpenGL renderer from a Glutin display.
    #[cfg(all(feature = "glutin", not(target_arch = "wasm32")))]
    pub fn new_from_glutin_display(display: &impl GlDisplay) -> Result<Self, ErrorKind> {
        unsafe { OpenGl::new_from_function_cstr(|s| display.get_proc_address(s).cast()) }
    }

    /// Creates a new OpenGL renderer from an HTML canvas element in a WASM32 target.
    #[cfg(wasm_unknown)]
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
        Self::new_from_context(context, true, true)
    }

    fn new_from_context(context: glow::Context, is_opengles_2_0: bool, indexed_fans: bool) -> Result<Self, ErrorKind> {
        let debug = cfg!(debug_assertions);
        let antialias = true;

        let context = Rc::new(context);
        // What the driver can allocate; a VideoCore IV reports 2048.
        let max_texture_size = match unsafe { context.get_parameter_i32(glow::MAX_TEXTURE_SIZE) } {
            size if size > 0 => size as usize,
            _ => 2048,
        };

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
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillGradientTwoPointRadial,
                    with_glyph_texture,
                )?),
                Some(MainProgram::new(
                    &context,
                    antialias,
                    ShaderType::FillImageGradientTwoPointRadial,
                    with_glyph_texture,
                )?),
                if with_glyph_texture {
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageTurbulence,
                        false,
                    )?)
                },
                if with_glyph_texture {
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageTransfer,
                        false,
                    )?)
                },
                // The blend pass samples its backdrop through the glyph
                // texture, so it only exists in that variant.
                if with_glyph_texture {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageBlend,
                        true,
                    )?)
                } else {
                    None
                },
                if with_glyph_texture {
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageMorphology,
                        false,
                    )?)
                },
                if with_glyph_texture {
                    None
                } else {
                    Some(MainProgram::new(
                        &context,
                        antialias,
                        ShaderType::FilterImageOffset,
                        false,
                    )?)
                },
                if with_glyph_texture {
                    None
                } else {
                    Some(MainProgram::new(&context, antialias, ShaderType::FillBlur, false)?)
                },
            ])
        };

        let main_programs_with_glyph_texture = generate_shader_program_variants(true)?;
        let main_programs_without_glyph_texture = generate_shader_program_variants(false)?;
        // One of them now: a driver that cannot build the clip shape's code fails here, not at a draw.
        let mut clip_programs = FnvHashMap::default();
        clip_programs.insert(
            (ShaderType::FillColor.to_u8(), false, (true, true)),
            Some(MainProgram::with_clip(
                &context,
                antialias,
                ShaderType::FillColor,
                false,
                (true, true),
            )?),
        );

        let mut opengl = Self {
            debug,
            antialias,
            is_opengles_2_0: false,
            view: [0.0, 0.0],
            screen_view: [0.0, 0.0],
            main_programs_with_glyph_texture,
            main_programs_without_glyph_texture,
            clip_programs,
            current_clip_bounds: None,
            current_program: 0,
            current_program_needs_glyph_texture: true,
            current_program_clips: (false, false),
            vert_arr: None,
            vert_buff: None,
            fan_indices: FanIndices::default(),
            fan_index_buff: None,
            framebuffers: HashMap::default(),
            context,
            screen_target: None,
            current_render_target: RenderTarget::Screen,
            max_texture_size,
            screen_copy: None,
            warned_blur_skipped: false,
        };

        unsafe {
            opengl.is_opengles_2_0 = is_opengles_2_0;

            opengl.vert_arr = opengl.context.create_vertex_array().ok();
            opengl.vert_buff = opengl.context.create_buffer().ok();
            if indexed_fans {
                opengl.fan_index_buff = opengl.context.create_buffer().ok();
            }
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

    /// Enables the persistent-clip stencil test for plain (non-stencil)
    /// draws: pass only where the clip bit (0x80) is set. The winding bits
    /// stay untouched (write mask 0).
    fn begin_clip_guard(&self, active: bool) {
        if !active {
            return;
        }
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.stencil_mask(0x00);
            self.context.stencil_func(glow::EQUAL, 0x80, 0x80);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
        }
    }

    fn end_clip_guard(&self, active: bool) {
        if !active {
            return;
        }
        unsafe {
            self.context.stencil_mask(0xff);
            self.context.disable(glow::STENCIL_TEST);
        }
    }

    /// Intersects the stencil clip with the command's path: winding
    /// accumulates only where the current clip bit is set (so nesting
    /// intersects for free), then one quad clears the clip bit wherever no
    /// winding arrived and a second clears the winding bits.
    fn clip_fill(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, stencil_paint: &Params) {
        self.set_uniforms(images, stencil_paint, None, GlyphTexture::None);
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.color_mask(false, false, false, false);

            // Winding, gated on the current clip bit; bit 7 is write-protected.
            self.context.stencil_mask(0x7f);
            self.context.stencil_func(glow::EQUAL, 0x80, 0x80);
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

            let winding_mask = match cmd.fill_rule {
                FillRule::NonZero => 0xff,
                FillRule::EvenOdd => 0x81,
            };
            if let Some((start, count)) = cmd.triangles_verts {
                // Quad 1: clip bit set but no winding -> outside the new clip.
                self.context.stencil_mask(0x80);
                self.context.stencil_func(glow::EQUAL, 0x80, winding_mask);
                self.context.stencil_op(glow::KEEP, glow::KEEP, glow::ZERO);
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);

                // Quad 2: clear the winding bits.
                self.context.stencil_mask(0x7f);
                self.context.stencil_func(glow::ALWAYS, 0x0, 0xff);
                self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
            }

            self.context.stencil_mask(0xff);
            self.context.color_mask(true, true, true, true);
            self.context.disable(glow::STENCIL_TEST);
        }
        self.check_error("clip_fill");
    }

    /// Resets the stencil clip to "everything visible" (0x80 everywhere).
    fn clip_reset(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, stencil_paint: &Params, visible: bool) {
        self.set_uniforms(images, stencil_paint, None, GlyphTexture::None);
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.color_mask(false, false, false, false);
            self.context.stencil_mask(0xff);
            self.context
                .stencil_func(glow::ALWAYS, if visible { 0x80 } else { 0x00 }, 0xff);
            self.context.stencil_op(glow::REPLACE, glow::REPLACE, glow::REPLACE);
            if let Some((start, count)) = cmd.triangles_verts {
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
            }
            self.context.color_mask(true, true, true, true);
            self.context.disable(glow::STENCIL_TEST);
        }
        self.check_error("clip_reset");
    }

    /// Draws the fan of `count` vertices from `start`: as triangles of the
    /// frame's fan indices from `first`, moved past them, or without fan
    /// indices as a fan.
    fn draw_fan(&self, first: &mut Option<u32>, start: usize, count: usize) {
        unsafe {
            match first {
                Some(first) => {
                    let indices = FanIndices::count(count);
                    let offset = (*first as usize * mem::size_of::<u32>()) as i32;
                    self.context
                        .draw_elements(glow::TRIANGLES, indices as i32, glow::UNSIGNED_INT, offset);
                    *first += indices;
                }
                None => self.context.draw_arrays(glow::TRIANGLE_FAN, start as i32, count as i32),
            }
        }
    }

    fn convex_fill(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, fans: Option<u32>, gpu_paint: &Params) {
        self.begin_clip_guard(cmd.clip_active);
        self.set_uniforms(images, gpu_paint, cmd.image, cmd.glyph_texture);
        let mut first = fans;

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                self.draw_fan(&mut first, start, count);
            }

            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.end_clip_guard(cmd.clip_active);
        self.check_error("convex_fill");
    }

    fn concave_fill(
        &mut self,
        images: &ImageStore<GlTexture>,
        cmd: &Command,
        fans: Option<u32>,
        stencil_paint: &Params,
        fill_paint: &Params,
    ) {
        let clip = cmd.clip_active;
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            if clip {
                // Winding accumulates only inside the clip; bit 7 is protected.
                self.context.stencil_mask(0x7f);
                self.context.stencil_func(glow::EQUAL, 0x80, 0x80);
            } else {
                self.context.stencil_mask(0xff);
                self.context.stencil_func(glow::ALWAYS, 0, 0xff);
            }
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

        let mut first = fans;
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                self.draw_fan(&mut first, start, count);
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
                match (clip, cmd.fill_rule) {
                    (false, FillRule::NonZero) => self.context.stencil_func(glow::EQUAL, 0x0, 0xff),
                    (false, FillRule::EvenOdd) => self.context.stencil_func(glow::EQUAL, 0x0, 0x1),
                    // Fringe where the clip bit is set and no winding arrived.
                    (true, FillRule::NonZero) => self.context.stencil_func(glow::EQUAL, 0x80, 0xff),
                    (true, FillRule::EvenOdd) => self.context.stencil_func(glow::EQUAL, 0x80, 0x81),
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
            match (clip, cmd.fill_rule) {
                (false, FillRule::NonZero) => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0xff),
                (false, FillRule::EvenOdd) => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0x1),
                // Cover where winding arrived: only in-clip pixels can hold
                // winding, and 0x80|w > 0x80 exactly when w != 0. LESS passes
                // when ref < stencil, and the ops below (ZERO, masked to the
                // winding bits) clear the winding while protecting the clip
                // bit.
                (true, FillRule::NonZero) => self.context.stencil_func(glow::LESS, 0x80, 0xff),
                (true, FillRule::EvenOdd) => self.context.stencil_func(glow::LESS, 0x80, 0x81),
            }
            if clip {
                self.context.stencil_mask(0x7f);
            }

            self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);

            if let Some((start, count)) = cmd.triangles_verts {
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
            }

            self.context.stencil_mask(0xff);
            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("concave_fill");
    }

    fn stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.begin_clip_guard(cmd.clip_active);
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.end_clip_guard(cmd.clip_active);
        self.check_error("stroke");
    }

    fn stencil_stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint1: &Params, paint2: &Params) {
        unsafe {
            self.context.enable(glow::STENCIL_TEST);

            // Fill the stroke base without overlap; with a clip active the
            // untouched in-clip value is 0x80 rather than 0, and the counter
            // increments stay inside the winding bits.
            if cmd.clip_active {
                self.context.stencil_mask(0x7f);
                self.context.stencil_func(glow::EQUAL, 0x80, 0xff);
            } else {
                self.context.stencil_mask(0xff);
                self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            }
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
            if cmd.clip_active {
                self.context.stencil_func(glow::EQUAL, 0x80, 0xff);
            } else {
                self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            }
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
            // Clear the stroke counters (the clip bit is write-protected).
            self.context.color_mask(false, false, false, false);
            if cmd.clip_active {
                self.context.stencil_mask(0x7f);
            }
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
            self.context.stencil_mask(0xff);
            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("stencil_stroke");
    }

    fn triangles(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.begin_clip_guard(cmd.clip_active);
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe {
                self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
            }
        }

        self.end_clip_guard(cmd.clip_active);
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

        if let Some(mask) = paint.clip_mask {
            let cliptex = images.get(mask.image).map(GlTexture::id);
            unsafe {
                self.context.active_texture(glow::TEXTURE0 + 2);
                self.context.bind_texture(glow::TEXTURE_2D, cliptex);
            }
        }

        self.check_error("set_uniforms texture");
    }

    fn warn_skipped(&mut self, reason: &str) {
        if !std::mem::replace(&mut self.warned_blur_skipped, true) {
            log::warn!("blur skipped: {reason}");
        }
    }

    /// Copies `src` of the screen into `screen_copy` at the same place and
    /// returns the copy's texture.
    fn copy_screen(&mut self, [x, y, width, height]: [u32; 4]) -> Option<<glow::Context as glow::HasContext>::Texture> {
        // glCopyTexSubImage2D can't resolve a multisampled framebuffer.
        if unsafe { self.context.get_parameter_i32(glow::SAMPLE_BUFFERS) } > 0 {
            self.warn_skipped("the screen framebuffer is multisampled");
            return None;
        }
        // OpenGL ES can't copy a channel the framebuffer lacks, so an
        // alphaless screen is copied into an RGB texture, which samples as
        // opaque. Desktop GL fills the missing alpha with 1 itself.
        let format =
            if self.context.version().is_embedded && unsafe { self.context.get_parameter_i32(glow::ALPHA_BITS) } == 0 {
                PixelFormat::Rgb8
            } else {
                PixelFormat::Rgba8
            };
        let size = self.view.map(|extent| extent as usize);
        if self.screen_copy.as_ref().is_none_or(|copy| {
            let info = copy.info();
            [info.width(), info.height()] != size || info.format() != format
        }) {
            if let Some(copy) = self.screen_copy.take() {
                copy.delete(&self.context);
            }
            let info = ImageInfo::new(ImageFlags::PREMULTIPLIED, size[0], size[1], format);
            match GlTexture::new(&self.context, info, self.is_opengles_2_0) {
                Ok(copy) => self.screen_copy = Some(copy),
                Err(err) => {
                    self.warn_skipped(&format!("can't create the screen copy: {err:?}"));
                    return None;
                }
            }
        }
        let texture = self.screen_copy.as_ref()?.id();
        // The screen holds its rows bottom-up, as the copy does.
        let row = self.view[1] as i32 - (y + height) as i32;
        unsafe {
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.context.copy_tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x as i32,
                row,
                x as i32,
                row,
                width as i32,
                height as i32,
            );
            self.context.bind_texture(glow::TEXTURE_2D, None);
        }
        self.check_error("copy_screen");
        Some(texture)
    }

    fn clear_rect(&self, x: u32, y: u32, width: u32, height: u32, color: Color, keep_clip: bool) {
        unsafe {
            self.context.enable(glow::SCISSOR_TEST);
            self.context.scissor(
                x as i32,
                self.view[1] as i32 - (height as i32 + y as i32),
                width as i32,
                height as i32,
            );
            self.context.clear_color(color.r, color.g, color.b, color.a);
            // The stencil carries the clip plane in bit 7 (armed and disarmed
            // by the ClipReset quads) and the winding scratch in the rest. A
            // winding count a cover pass missed (a fan vertex pushed past the
            // cover quad at a cusp) must not leak into the next frame's
            // fills, so the stencil is cleared with the color. With a clip
            // armed on this target only the winding bits go, through the
            // write mask glClear honours; without one the whole stencil is
            // cleared, which a tiler does as a tile clear where a masked
            // stencil clear costs a full-target quad (Mesa's state tracker).
            self.context.stencil_mask(if keep_clip { 0x7f } else { 0xff });
            self.context.clear_stencil(0);
            self.context.clear(glow::COLOR_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            self.context.stencil_mask(0xff);
            self.context.disable(glow::SCISSOR_TEST);
        }
    }

    /// Limits the draws that follow to `crop` - x, y, width and height in
    /// the target's texel rows - until [`end_crop`](Self::end_crop), and says
    /// whether that leaves anything to draw.
    fn begin_crop(&self, crop: Option<[u32; 4]>) -> bool {
        let Some([x, y, width, height]) = crop else {
            return true;
        };
        if width == 0 || height == 0 {
            return false;
        }
        unsafe {
            self.context.enable(glow::SCISSOR_TEST);
            self.context.scissor(x as i32, y as i32, width as i32, height as i32);
        }
        true
    }

    fn end_crop(&self, crop: Option<[u32; 4]>) {
        if crop.is_some() {
            unsafe {
                self.context.disable(glow::SCISSOR_TEST);
            }
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
            ImageFilter::GaussianBlur { sigma_x, sigma_y } => {
                self.render_gaussian_blur(images, cmd, target_image, [sigma_x, sigma_y])
            }
            ImageFilter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => self.render_morphology(images, cmd, target_image, [radius_x, radius_y], operator),
            single_pass => {
                let target_image_info = images.get(target_image).unwrap().info();
                let (shader_type, slots) = single_pass
                    .single_pass(target_image_info.width() as f32, target_image_info.height() as f32)
                    .expect("every filter but the Gaussian blur and the morphology runs as one pass");
                self.render_filter_pass(images, cmd, target_image, shader_type, slots)
            }
        }
    }

    /// Runs a one-pass filter (color matrix, turbulence, transfer) over the
    /// command's quad into `target_image`, sampling the command's image.
    fn render_filter_pass(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        shader_type: ShaderType,
        slots: [f32; 20],
    ) {
        let original_render_target = self.current_render_target;
        let source_image_info = images.get(cmd.image.unwrap()).unwrap().info();
        let target_image_info = images.get(target_image).unwrap().info();

        let image_paint = crate::Paint::image(
            cmd.image.unwrap(),
            0.,
            0.,
            source_image_info.width() as _,
            source_image_info.height() as _,
            0.,
            1.,
        );
        // A blend pass binds its backdrop in the glyph-texture slot; every
        // other pass binds nothing there.
        let mut params = Params::new(
            images,
            &Transform2D::default(),
            &image_paint.flavor,
            &cmd.glyph_texture,
            &Scissor::default(),
            0.,
            0.,
            0.,
        );
        params.shader_type = shader_type;
        let mut slots = slots;
        if shader_type == ShaderType::FilterImageBlend {
            let pass = cmd.blend_pass;
            slots[1] = f32::from(u8::from(pass.backdrop_flipped));
            slots[2] = pass.source_alpha;
            slots[3] = f32::from(u8::from(pass.contribution));
        }
        // The filter's parameters ride the scissor/paint-matrix uniform slots,
        // which are dead during a filter pass (no scissor, no paint gradient):
        // frag[0..2] hold the first 12 values, frag[3..4] the last 8, so no
        // uniform-array growth is needed (see `ImageFilter::single_pass`).
        params.scissor_mat.copy_from_slice(&slots[..12]);
        params.paint_mat[..8].copy_from_slice(&slots[12..20]);
        // A generating pass binds its lookup table as the image, so the output
        // extent comes from the target rather than from what is sampled.
        params.extent = [target_image_info.width() as f32, target_image_info.height() as f32];

        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program().set_view(self.view);
        self.clear_rect(
            0,
            0,
            target_image_info.width() as _,
            target_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
            false,
        );
        if self.begin_crop(cmd.crop) {
            self.triangles(images, &cmd, &params);
        }
        self.end_crop(cmd.crop);

        self.set_target(images, original_render_target);
        self.main_program().set_view(self.view);
    }

    /// A filter of two draws along the axes: the horizontal one, with the
    /// parameters `axis_params(params, 0)` sets, into the canvas-owned
    /// scratch, then the vertical one, with `axis_params(params, 1)`, from
    /// the scratch into `target_image`.
    fn render_separable_filter(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        mut cmd: Command,
        target_image: ImageId,
        mut axis_params: impl FnMut(&mut Params, usize),
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
        axis_params(&mut params, 0);
        params.image_blur_filter_direction = [1.0, 0.0];
        // The fused matrices ride parameters a filter draw leaves unused: an
        // alpha-only matrix before the filter is the `radius` flag on the
        // first draw, a matrix after it the `feather` flag and the matrix
        // slots on the second.
        params.radius = f32::from(u8::from(cmd.fused.source_alpha));

        let horizontal_buffer = cmd
            .filter_scratch
            .expect("a two-pass filter has a reserved scratch image");
        self.set_target(images, RenderTarget::Image(horizontal_buffer));
        self.main_program().set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
            false,
        );

        self.triangles(images, &cmd, &params);

        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program().set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
            false,
        );

        axis_params(&mut params, 1);
        params.image_blur_filter_direction = [0.0, 1.0];
        // The horizontal draw stored premultiplied RGBA regardless of the
        // source image's format or premultiplication flag.
        params.tex_type = 0.0;
        params.radius = 0.0;
        if let Some(matrix) = cmd.fused.post_matrix {
            params.scissor_mat.copy_from_slice(&matrix[..12]);
            params.paint_mat[..8].copy_from_slice(&matrix[12..20]);
            params.feather = 1.0;
        }

        cmd.image = Some(horizontal_buffer);

        if self.begin_crop(cmd.crop) {
            self.triangles(images, &cmd, &params);
        }
        self.end_crop(cmd.crop);

        // restore previous render target and view
        self.set_target(images, original_render_target);
        self.main_program().set_view(self.view);
    }

    /// Draws `src` of the current render target over `target_image`: see
    /// `CommandType::StartBlur`. Returns whether it drew.
    fn start_blur(
        &mut self,
        images: &ImageStore<GlTexture>,
        cmd: &Command,
        target_image: ImageId,
        src: [u32; 4],
        sigma: f32,
        sample_transform: Transform2D,
    ) -> bool {
        let Some(target_info) = images.get(target_image).map(GlTexture::info) else {
            return false;
        };
        // An image target is read where it is; the screen through a copy.
        let (source, size, is_image) = match self.current_render_target {
            RenderTarget::Image(id) => {
                let Some(texture) = images.get(id) else {
                    return false;
                };
                let info = texture.info();
                (texture.id(), [info.width() as f32, info.height() as f32], true)
            }
            RenderTarget::Screen => {
                let Some(copy) = self.copy_screen(src) else {
                    return false;
                };
                (copy, self.view, false)
            }
        };
        let (target_width, target_height) = (target_info.width(), target_info.height());
        // Every OpenGL target holds its rows bottom-up.
        let flip_y = Transform2D([1.0, 0.0, 0.0, -1.0, 0.0, size[1]]);
        let params = Params::blur_axis(size, sigma, [1.0, 0.0], &(sample_transform * flip_y));

        let original_render_target = self.current_render_target;
        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program().set_view(self.view);
        self.clear_rect(
            0,
            0,
            target_width as u32,
            target_height as u32,
            Color::rgbaf(0., 0., 0., 0.),
            false,
        );
        self.set_uniforms(images, &params, None, GlyphTexture::None);
        unsafe {
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(glow::TEXTURE_2D, Some(source));
            // The taps clamp and the first halving averages only through a
            // linear, clamped lookup of the base level, whatever the image's
            // own flags (or, for a native texture, its owner) set.
            let saved = is_image.then(|| {
                RECT_BLUR_SAMPLING.map(|(parameter, value)| {
                    let previous = self.context.get_tex_parameter_i32(glow::TEXTURE_2D, parameter);
                    self.context
                        .tex_parameter_i32(glow::TEXTURE_2D, parameter, value as i32);
                    (parameter, previous)
                })
            });
            if let Some((start, count)) = cmd.triangles_verts {
                self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
            }
            for (parameter, previous) in saved.into_iter().flatten() {
                self.context.tex_parameter_i32(glow::TEXTURE_2D, parameter, previous);
            }
        }
        self.check_error("start_blur");

        self.set_target(images, original_render_target);
        self.main_program().set_view(self.view);
        true
    }

    /// Two-pass Gaussian blur of `cmd.image` into `target_image`: horizontal
    /// by `sigma[0]`, then vertical by `sigma[1]`. A degenerate sigma makes
    /// that draw a copy.
    fn render_gaussian_blur(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        sigma: [f32; 2],
    ) {
        self.render_separable_filter(images, cmd, target_image, |params, axis| {
            params.shader_type = ShaderType::FilterImage;
            let (coeff, sigma) = crate::renderer::gaussian_blur_coefficients(sigma[axis]);
            params.image_blur_filter_coeff[..3].copy_from_slice(&coeff);
            params.image_blur_filter_sigma = sigma;
        });
    }

    /// Two-pass morphology of `cmd.image` into `target_image`: the
    /// per-channel maximum (dilate) or minimum (erode) over `radius[0]`
    /// whole pixels either side along x, then over `radius[1]` along y. The
    /// radius rides the blur's sigma parameter and the operator its first
    /// coefficient.
    fn render_morphology(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        radius: [f32; 2],
        operator: crate::MorphologyOperator,
    ) {
        self.render_separable_filter(images, cmd, target_image, |params, axis| {
            params.shader_type = ShaderType::FilterImageMorphology;
            params.image_blur_filter_sigma = radius[axis];
            params.image_blur_filter_coeff[0] = f32::from(u8::from(operator == crate::MorphologyOperator::Dilate));
        });
    }

    /// Scissors the draws that follow to `bounds` - x, y, width and height
    /// from the target's top left - or lifts the scissor.
    fn set_clip_bounds(&mut self, bounds: Option<[u32; 4]>) {
        if self.current_clip_bounds == bounds {
            return;
        }
        self.current_clip_bounds = bounds;
        unsafe {
            match bounds {
                Some([x, y, width, height]) => {
                    self.context.enable(glow::SCISSOR_TEST);
                    self.context.scissor(
                        x as i32,
                        self.view[1] as i32 - (y + height) as i32,
                        width as i32,
                        height as i32,
                    );
                }
                None => self.context.disable(glow::SCISSOR_TEST),
            }
        }
    }

    fn main_program(&self) -> &MainProgram {
        if self.current_program_clips != (false, false) {
            return self
                .clip_programs
                .get(&(
                    self.current_program,
                    self.current_program_needs_glyph_texture,
                    self.current_program_clips,
                ))
                .and_then(Option::as_ref)
                .expect("internal error: clip program selected before it was built");
        }
        let programs = if self.current_program_needs_glyph_texture {
            &self.main_programs_with_glyph_texture
        } else {
            &self.main_programs_without_glyph_texture
        };
        programs[self.current_program as usize]
            .as_ref()
            .expect("internal error: invalid shader program selected for given paint")
    }

    /// Whether the program for `params` under `clips` - a clip shape, a coverage mask - is there, building it when
    /// first asked for. Start-up built one, so the driver can; what fails later is a lost context, which draws
    /// nothing with any program.
    fn has_clip_program(&mut self, params: &Params, clips: (bool, bool)) -> bool {
        let (context, antialias) = (&self.context, self.antialias);
        let (shader_type, with_glyph_texture) = (params.shader_type, params.uses_glyph_texture());
        self.clip_programs
            .entry((shader_type.to_u8(), with_glyph_texture, clips))
            .or_insert_with(|| {
                MainProgram::with_clip(context, antialias, shader_type, with_glyph_texture, clips)
                    .map_err(|error| log::error!("clip program for {shader_type:?}: {error}"))
                    .ok()
            })
            .is_some()
    }

    fn select_main_program(&mut self, params: &Params) {
        let program_index = params.shader_type.to_u8();
        let mut clips = (params.clip.is_some(), params.clip_mask.is_some());
        if clips != (false, false) && !self.has_clip_program(params, clips) {
            clips = (false, false);
        }
        if program_index != self.current_program
            || params.uses_glyph_texture() != self.current_program_needs_glyph_texture
            || clips != self.current_program_clips
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
            self.current_program_clips = clips;

            let program = self.main_program();
            program.bind();
            // Bind the two uniform samplers to texture units
            program.set_tex(0);
            program.set_glyphtex(1);
            program.set_cliptex(2);
            program.set_view(self.view);
        }
    }
}

impl Renderer for OpenGl {
    type Image = GlTexture;
    #[cfg(not(wasm_unknown))]
    type NativeTexture = <glow::Context as glow::HasContext>::Texture;
    /// The `WebGlTexture` must belong to the WebGL context of the canvas this renderer draws into.
    #[cfg(wasm_unknown)]
    type NativeTexture = web_sys::WebGlTexture;
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
        #[cfg(not(wasm_unknown))]
        return Ok(image.id());
        #[cfg(wasm_unknown)]
        return self
            .context
            .as_web_gl_texture(image.id())
            .ok_or(ErrorKind::ImageIdNotFound);
    }

    fn render(
        &mut self,
        _output: impl Into<Self::RenderOutput>,
        images: &mut ImageStore<Self::Image>,
        verts: &[Vertex],
        commands: Vec<Command>,
    ) {
        self.current_program = 0;
        self.current_program_clips = (false, false);
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

            if self.fan_index_buff.is_some() {
                self.fan_indices.build(&commands);
                // Bound with the vertex array, which keeps it for the frame.
                self.context
                    .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, self.fan_index_buff);
                self.context.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    self.fan_indices.indices().align_to().1,
                    glow::STREAM_DRAW,
                );
            }

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

        // Whether the last `StartBlur` drew.
        let mut sampled = false;
        for (index, cmd) in commands.into_iter().enumerate() {
            self.set_clip_bounds(cmd.clip_bounds([self.view[0] as u32, self.view[1] as u32]));
            self.set_composite_operation(cmd.composite_operation);
            let fans = self.fan_index_buff.map(|_| self.fan_indices.firsts[index]);

            match cmd.cmd_type {
                CommandType::ConvexFill { ref params } => self.convex_fill(images, &cmd, fans, params),
                CommandType::ConcaveFill {
                    ref stencil_params,
                    ref fill_params,
                } => self.concave_fill(images, &cmd, fans, stencil_params, fill_params),
                CommandType::Stroke { ref params } => self.stroke(images, &cmd, params),
                CommandType::StencilStroke {
                    ref params1,
                    ref params2,
                } => self.stencil_stroke(images, &cmd, params1, params2),
                CommandType::Triangles { ref params } => self.triangles(images, &cmd, params),
                CommandType::ClearRect { color, keep_clip } => {
                    if let Some((start, _)) = cmd.triangles_verts {
                        let x = verts[start].x as _;
                        let y = verts[start].y as _;
                        let width = verts[start + 1].x as u32 - x;
                        let height = verts[start + 1].y as u32 - y;
                        self.clear_rect(x, y, width, height, color, keep_clip);
                    }
                }
                CommandType::SetRenderTarget(target) => {
                    self.set_target(images, target);
                    self.main_program().set_view(self.view);
                }
                CommandType::RenderFilteredImage { target_image, filter } => {
                    self.render_filtered_image(images, cmd, target_image, filter)
                }
                CommandType::StartBlur {
                    target_image,
                    src,
                    sigma,
                    sample_transform,
                } => sampled = self.start_blur(images, &cmd, target_image, src, sigma, sample_transform),
                CommandType::FinishBlur { ref stencil_params, .. } => {
                    // Its source is the first draw's image, or that image run
                    // down a pyramid: without that draw it would write back
                    // whatever the image held before.
                    if sampled {
                        for (composite_operation, params) in cmd.finish_blur_draws() {
                            self.set_composite_operation(composite_operation);
                            match stencil_params {
                                Some(stencil_params) => {
                                    self.concave_fill(images, &cmd, fans, stencil_params, &params);
                                }
                                None => self.convex_fill(images, &cmd, fans, &params),
                            }
                        }
                    }
                }
                CommandType::ClipFill => {
                    let stencil_params = Params::stencil();
                    self.clip_fill(images, &cmd, &stencil_params);
                }
                CommandType::ClipReset { visible } => {
                    let stencil_params = Params::stencil();
                    self.clip_reset(images, &cmd, &stencil_params, visible);
                }
            }
        }

        // No scissor of a clipped draw outlives the frame.
        self.set_clip_bounds(None);

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
        // glow's web backend refers to textures by keys into a table of the `glow::Context`,
        // so a texture created by another `glow::Context` or by JavaScript needs a key in ours.
        #[cfg(wasm_unknown)]
        let native_texture = unsafe { self.context.register_external_texture(native_texture) };
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

    fn max_texture_size(&self) -> usize {
        self.max_texture_size
    }

    fn transient_image_cost(&self, info: ImageInfo) -> usize {
        opengl_transient_image_cost(info)
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

fn opengl_transient_image_cost(info: ImageInfo) -> usize {
    let color_bytes: usize = match info.format() {
        crate::PixelFormat::Gray8 => 1,
        crate::PixelFormat::Rgb8 | crate::PixelFormat::Rgba8 => 4,
    };
    // Color plus the target's Stencil8 attachment. Gaussian blur scratch is
    // a separate transient image and is charged independently.
    info.width()
        .saturating_mul(info.height())
        .saturating_mul(color_bytes + 1)
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

        for buffer in [self.vert_buff, self.fan_index_buff].into_iter().flatten() {
            unsafe {
                self.context.delete_buffer(buffer);
            }
        }
        if let Some(copy) = self.screen_copy.take() {
            copy.delete(&self.context);
        }
    }
}

#[cfg(test)]
mod transient_cost_tests {
    use super::opengl_transient_image_cost;
    use crate::{ImageFlags, ImageInfo, PixelFormat};

    #[test]
    fn charges_color_and_stencil() {
        let pixels = 1242 * 2688;
        let rgba = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Rgba8);
        let rgb = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Rgb8);
        let gray = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Gray8);

        assert_eq!(opengl_transient_image_cost(rgba), pixels * 5);
        assert_eq!(opengl_transient_image_cost(rgb), pixels * 5);
        assert_eq!(opengl_transient_image_cost(gray), pixels * 2);
    }
}
