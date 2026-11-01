use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rgb::bytemuck;
use wgpu::util::DeviceExt;

use crate::image::ImageStore;
use crate::paint::GlyphTexture;
use crate::renderer::ShaderType;
use crate::BlendFactor;
use crate::FillRule;
use crate::ImageId;
use crate::ImageInfo;
use crate::RenderTarget;
use crate::Scissor;

use super::Renderer;

pub use wgpu;

/// Describes the render surface for the WGPU renderer.
///
/// Bundles a [`wgpu::TextureView`] with the metadata needed for rendering.
/// Using a view instead of a texture allows rendering into specific mip levels
/// or array layers, and reinterpreting the texture format (e.g. linear vs sRGB).
#[derive(Clone)]
pub struct WGPURenderOutput {
    /// The texture view to render into.
    pub view: wgpu::TextureView,
    /// Width of the render target in pixels.
    pub width: u32,
    /// Height of the render target in pixels.
    pub height: u32,
    /// Texture format of the render target.
    pub format: wgpu::TextureFormat,
}

impl From<&wgpu::Texture> for WGPURenderOutput {
    fn from(texture: &wgpu::Texture) -> Self {
        let size = texture.size();
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            width: size.width,
            height: size.height,
            format: texture.format(),
        }
    }
}

impl std::fmt::Debug for WGPURenderOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WGPURenderOutput")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

use super::Params;
use super::Vertex;

const UNIFORMARRAY_SIZE: usize = 14;
const UNIFORM_BYTES: u64 = (UNIFORMARRAY_SIZE * 4 * 4) as u64;
// A concave fill and a stencil stroke record two sets of params, every other command one.
const UNIFORM_SLOTS_PER_COMMAND: u64 = 2;
const MIN_UNIFORM_SLOTS: u64 = 64;
const MIN_VERTEX_BYTES: u64 = 4096;

const UNIFORM_BUFFER_LABEL: &str = "Fragment Uniform Buffer";
const UNIFORM_BUFFER_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::UNIFORM.union(wgpu::BufferUsages::COPY_DST);
const VERTEX_BUFFER_LABEL: &str = "Main Vertex Buffer";
const VERTEX_BUFFER_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::VERTEX.union(wgpu::BufferUsages::COPY_DST);

/// Replaces `buffer` with a larger one when `needed` bytes no longer fit.
fn grow_buffer(device: &wgpu::Device, buffer: &mut wgpu::Buffer, needed: u64, label: &str, usage: wgpu::BufferUsages) {
    if buffer.size() >= needed {
        return;
    }

    *buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: needed.next_power_of_two(),
        usage,
        mapped_at_creation: false,
    });
}

#[derive(Clone, PartialEq)]
pub struct UniformArray([f32; UNIFORMARRAY_SIZE * 4]);

impl Default for UniformArray {
    fn default() -> Self {
        Self([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ])
    }
}

impl UniformArray {
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    pub fn set_scissor_mat(&mut self, mat: [f32; 12]) {
        self.0[0..12].copy_from_slice(&mat);
    }

    pub fn set_paint_mat(&mut self, mat: [f32; 12]) {
        self.0[12..24].copy_from_slice(&mat);
    }

    pub fn set_inner_col(&mut self, col: [f32; 4]) {
        self.0[24..28].copy_from_slice(&col);
    }

    pub fn set_outer_col(&mut self, col: [f32; 4]) {
        self.0[28..32].copy_from_slice(&col);
    }

    pub fn set_scissor_ext(&mut self, ext: [f32; 2]) {
        self.0[32..34].copy_from_slice(&ext);
    }

    pub fn set_scissor_scale(&mut self, scale: [f32; 2]) {
        self.0[34..36].copy_from_slice(&scale);
    }

    pub fn set_scissor_radius(&mut self, radius: f32) {
        self.0[51] = radius;
    }

    pub fn set_extent(&mut self, ext: [f32; 2]) {
        self.0[36..38].copy_from_slice(&ext);
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.0[38] = radius;
    }

    pub fn set_feather(&mut self, feather: f32) {
        self.0[39] = feather;
    }

    pub fn set_stroke_mult(&mut self, stroke_mult: f32) {
        self.0[40] = stroke_mult;
    }

    pub fn set_stroke_thr(&mut self, stroke_thr: f32) {
        self.0[41] = stroke_thr;
    }

    pub fn set_tex_type(&mut self, tex_type: f32) {
        self.0[42] = tex_type;
    }

    pub fn set_shader_type(&mut self, shader_type: f32) {
        self.0[43] = shader_type;
    }

    pub fn set_glyph_texture_type(&mut self, glyph_texture_type: f32) {
        self.0[44] = glyph_texture_type;
    }

    pub fn set_image_blur_filter_direction(&mut self, direction: [f32; 2]) {
        self.0[46..48].copy_from_slice(&direction);
    }

    pub fn set_image_blur_filter_sigma(&mut self, sigma: f32) {
        self.0[45] = sigma;
    }

    pub fn set_image_blur_filter_coeff(&mut self, coeff: [f32; 3]) {
        self.0[48..51].copy_from_slice(&coeff);
    }

    pub fn set_conic_start_angle(&mut self, angle: f32) {
        // Byte offset 208 (`conic_start_angle` in the WGSL Params struct);
        // float 51 (byte offset 204) holds the scissor radius.
        self.0[52] = angle;
    }
}

impl From<&Params> for UniformArray {
    fn from(params: &Params) -> Self {
        let mut arr = Self::default();

        arr.set_scissor_mat(params.scissor_mat);
        arr.set_paint_mat(params.paint_mat);
        arr.set_inner_col(params.inner_col);
        arr.set_outer_col(params.outer_col);
        arr.set_scissor_ext(params.scissor_ext);
        arr.set_scissor_scale(params.scissor_scale);
        arr.set_scissor_radius(params.scissor_radius);
        arr.set_extent(params.extent);
        arr.set_radius(params.radius);
        arr.set_feather(params.feather);
        arr.set_stroke_mult(params.stroke_mult);
        arr.set_stroke_thr(params.stroke_thr);
        arr.set_shader_type(params.shader_type.to_f32());
        arr.set_tex_type(params.tex_type);
        arr.set_glyph_texture_type(params.glyph_texture_type as f32);
        arr.set_image_blur_filter_direction(params.image_blur_filter_direction);
        arr.set_image_blur_filter_sigma(params.image_blur_filter_sigma);
        arr.set_image_blur_filter_coeff(params.image_blur_filter_coeff);
        arr.set_conic_start_angle(params.conic_start_angle);

        arr
    }
}

enum OwnedBindingResource {
    TextureView(wgpu::TextureView),
    ExternalTexture(wgpu::ExternalTexture),
}

impl OwnedBindingResource {
    fn is_external(&self) -> bool {
        matches!(self, Self::ExternalTexture(_))
    }
}

impl<'a> From<&'a OwnedBindingResource> for wgpu::BindingResource<'a> {
    fn from(owned: &'a OwnedBindingResource) -> Self {
        match owned {
            OwnedBindingResource::TextureView(texture_view) => Self::TextureView(texture_view),
            OwnedBindingResource::ExternalTexture(texture_view) => Self::ExternalTexture(texture_view),
        }
    }
}

#[derive(Debug)]
enum Texture {
    Internal(wgpu::Texture),
    External(wgpu::ExternalTexture),
}

#[derive(Debug)]
pub struct Image {
    texture: Texture,
    info: ImageInfo,
}

// Only these flags change a sampler descriptor; the rest would split the cache for nothing.
const SAMPLER_FLAGS: crate::ImageFlags = crate::ImageFlags::REPEAT_X
    .union(crate::ImageFlags::REPEAT_Y)
    .union(crate::ImageFlags::NEAREST);

type SamplerCache = Rc<RefCell<HashMap<crate::ImageFlags, wgpu::Sampler>>>;

#[derive(Debug)]
struct CachedPipeline {
    pipeline: wgpu::RenderPipeline,
    accessed: bool,
}

/// WGPU renderer.
#[derive(Debug)]
pub struct WGPURenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,

    shader_module: Rc<wgpu::ShaderModule>,

    screen_view: [f32; 2],

    empty_texture_view: wgpu::TextureView,
    sampler_cache: SamplerCache,
    uniform_buffer: wgpu::Buffer,
    uniform_stride: u64,
    vertex_buffer: wgpu::Buffer,
    stencil_buffer: Option<wgpu::Texture>,
    stencil_buffer_for_textures: HashMap<wgpu::Texture, wgpu::Texture>,

    bind_group_layout: wgpu::BindGroupLayout,
    viewport_bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
}

/// Rasterizes an image element into an offscreen canvas at the given size.
#[cfg(target_arch = "wasm32")]
fn rasterize_to_canvas(
    element: &web_sys::HtmlImageElement,
    size: crate::image::Size,
) -> Result<web_sys::HtmlCanvasElement, crate::ErrorKind> {
    use wasm_bindgen::JsCast;

    let canvas = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.create_element("canvas").ok())
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or(crate::ErrorKind::UnsupportedOperation)?;
    canvas.set_width(size.width as u32);
    canvas.set_height(size.height as u32);

    let context = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|context| context.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
        .ok_or(crate::ErrorKind::UnsupportedOperation)?;
    context
        .draw_image_with_html_image_element_and_dw_and_dh(element, 0., 0., size.width as f64, size.height as f64)
        .map_err(|_| crate::ErrorKind::UnsupportedOperation)?;

    Ok(canvas)
}

impl WGPURenderer {
    /// Uploads a browser-side image source straight into an image's texture.
    #[cfg(target_arch = "wasm32")]
    fn copy_external_image(
        &self,
        image: &Image,
        source: wgpu::ExternalImageSource,
        size: crate::image::Size,
        x: usize,
        y: usize,
    ) -> Result<(), crate::ErrorKind> {
        let Texture::Internal(texture) = &image.texture else {
            return Err(crate::ErrorKind::UnsupportedOperation);
        };
        self.queue.copy_external_image_to_texture(
            &wgpu::CopyExternalImageSourceInfo {
                source,
                origin: wgpu::Origin2d::ZERO,
                flip_y: false,
            },
            wgpu::CopyExternalImageDestInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: x as u32,
                    y: y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
                color_space: wgpu::PredefinedColorSpace::Srgb,
                premultiplied_alpha: true,
            },
            wgpu::Extent3d {
                width: size.width as _,
                height: size.height as _,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }

    /// Creates a new renderer for the device.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let module = wgpu::include_wgsl!("wgpu/shader.wgsl");
        let shader_module = Rc::new(device.create_shader_module(module));

        let texture_descriptor = wgpu::TextureDescriptor {
            size: wgpu::Extent3d::default(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: None,
            view_formats: &[],
        };
        // A dynamic offset must be a multiple of this, usually 256.
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
        let uniform_stride = UNIFORM_BYTES.div_ceil(alignment) * alignment;

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(UNIFORM_BUFFER_LABEL),
            size: MIN_UNIFORM_SLOTS * uniform_stride,
            usage: UNIFORM_BUFFER_USAGE,
            mapped_at_creation: false,
        });
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(VERTEX_BUFFER_LABEL),
            size: MIN_VERTEX_BYTES,
            usage: VERTEX_BUFFER_USAGE,
            mapped_at_creation: false,
        });

        let empty_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("empty"),
            view_formats: &[],
            ..texture_descriptor
        });

        queue.write_texture(
            empty_texture.as_image_copy(),
            &[255, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d::default(),
        );

        let viewport_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bind Group Layout for Viewport uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&viewport_bind_group_layout), Some(&bind_group_layout)],
            immediate_size: 0,
        });

        Self {
            device,
            queue,

            shader_module,

            screen_view: [0.0, 0.0],

            empty_texture_view: empty_texture.create_view(&Default::default()),
            sampler_cache: Rc::new(RefCell::new(HashMap::new())),
            uniform_buffer,
            uniform_stride,
            vertex_buffer,
            stencil_buffer: None,
            stencil_buffer_for_textures: HashMap::new(),
            bind_group_layout,
            viewport_bind_group_layout,
            pipeline_layout,
            pipeline_cache: Default::default(),
        }
    }
}

impl Renderer for WGPURenderer {
    type Image = Image;
    type NativeTexture = wgpu::Texture;
    type ExternalTexture = wgpu::ExternalTexture;
    type RenderOutput = WGPURenderOutput;
    type CommandBuffer = Option<wgpu::CommandBuffer>;

    fn set_size(&mut self, _width: u32, _height: u32, _dpi: f32) {}

    fn render(
        &mut self,
        output: impl Into<Self::RenderOutput>,
        images: &mut crate::image::ImageStore<Self::Image>,
        verts: &[super::Vertex],
        commands: Vec<super::Command>,
    ) -> Self::CommandBuffer {
        if commands.is_empty() {
            return None;
        }

        // The bind groups recorded below hold this buffer, so it cannot grow mid-frame.
        let needed_slots = commands.len() as u64 * UNIFORM_SLOTS_PER_COMMAND;
        grow_buffer(
            &self.device,
            &mut self.uniform_buffer,
            needed_slots * self.uniform_stride,
            UNIFORM_BUFFER_LABEL,
            UNIFORM_BUFFER_USAGE,
        );

        let output = output.into();

        self.screen_view[0] = output.width as f32;
        self.screen_view[1] = output.height as f32;

        let texture_view = output.view.clone();

        let vertex_bytes: &[u8] = bytemuck::cast_slice(verts);
        let vertex_needed =
            (vertex_bytes.len() as u64).div_ceil(wgpu::COPY_BUFFER_ALIGNMENT) * wgpu::COPY_BUFFER_ALIGNMENT;
        grow_buffer(
            &self.device,
            &mut self.vertex_buffer,
            vertex_needed,
            VERTEX_BUFFER_LABEL,
            VERTEX_BUFFER_USAGE,
        );
        if !vertex_bytes.is_empty() {
            self.queue.write_buffer(&self.vertex_buffer, 0, vertex_bytes);
        }
        let vertex_buffer = self.vertex_buffer.clone();

        if let Some(stencil_buffer) = &self.stencil_buffer {
            if stencil_buffer.width() != output.width || stencil_buffer.height() != output.height {
                self.stencil_buffer = None;
            }
        }

        let stencil_buffer = self
            .stencil_buffer
            .get_or_insert_with(|| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Stencil buffer"),
                    size: wgpu::Extent3d {
                        width: output.width,
                        height: output.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Stencil8,
                    view_formats: &[],
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                })
            })
            .clone();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        let mut render_pass_builder = RenderPassBuilder::new(
            self.device.clone(),
            &mut encoder,
            output.format,
            self.screen_view,
            self.viewport_bind_group_layout.clone(),
            &mut self.stencil_buffer_for_textures,
            texture_view,
            stencil_buffer.clone(),
            vertex_buffer,
        );
        // Ensure that we have one initial render pass, in case the first command is not SetRenderTarget
        render_pass_builder.set_render_target_screen();

        let mut pipeline_and_bindgroup_mapper = CommandToPipelineAndBindGroupMapper::new(
            self.device.clone(),
            self.empty_texture_view.clone(),
            self.sampler_cache.clone(),
            self.uniform_buffer.clone(),
            self.uniform_stride,
            self.shader_module.clone(),
            self.bind_group_layout.clone(),
            self.pipeline_layout.clone(),
            self.pipeline_cache.clone(),
        );

        let mut current_render_target = RenderTarget::Screen;

        for command in commands {
            match command.cmd_type {
                super::CommandType::SetRenderTarget(render_target) => {
                    current_render_target = render_target;
                    match render_target {
                        RenderTarget::Screen => {
                            render_pass_builder.set_render_target_screen();
                        }
                        RenderTarget::Image(image_id) => {
                            render_pass_builder.set_render_target_image(images, image_id, wgpu::LoadOp::Load);
                        }
                    }
                }
                super::CommandType::ClearRect { color } => {
                    clear_rect(
                        images,
                        color,
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                    );
                }
                super::CommandType::ConvexFill { ref params } => {
                    convex_fill(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        params,
                        images,
                    );
                }
                super::CommandType::ConcaveFill {
                    ref stencil_params,
                    ref fill_params,
                } => {
                    concave_fill(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        stencil_params,
                        images,
                        fill_params,
                    );
                }
                super::CommandType::Stroke { params } => {
                    stroke(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        params,
                        images,
                    );
                }
                super::CommandType::StencilStroke { params1, params2 } => {
                    stencil_stroke(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        params2,
                        images,
                        params1,
                    );
                }
                super::CommandType::Triangles { ref params } => {
                    triangles(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        params,
                        images,
                    );
                }
                super::CommandType::RenderFilteredImage { target_image, filter } => match filter {
                    crate::ImageFilter::GaussianBlur { sigma } => {
                        gaussian_blur_filter(
                            &self.device,
                            &mut current_render_target,
                            images,
                            command,
                            sigma,
                            &mut render_pass_builder,
                            &mut pipeline_and_bindgroup_mapper,
                            target_image,
                        );
                    }
                    crate::ImageFilter::ColorMatrix { matrix } => {
                        color_matrix_filter(
                            &mut current_render_target,
                            images,
                            command,
                            matrix,
                            &mut render_pass_builder,
                            &mut pipeline_and_bindgroup_mapper,
                            target_image,
                        );
                    }
                },
            }
        }

        drop(render_pass_builder);

        // write_buffer is ordered ahead of the caller's submit.
        let uniform_staging = &pipeline_and_bindgroup_mapper.uniform_staging;
        debug_assert!(
            uniform_staging.len() as u64 <= self.uniform_buffer.size(),
            "a command recorded more than UNIFORM_SLOTS_PER_COMMAND uniform slots"
        );
        if !uniform_staging.is_empty() {
            self.queue.write_buffer(&self.uniform_buffer, 0, uniform_staging);
        }

        let command_buffer = encoder.finish();

        self.pipeline_cache
            .borrow_mut()
            .retain(|_, cached_pipeline| std::mem::replace(&mut cached_pipeline.accessed, false));

        Some(command_buffer)
    }

    fn alloc_image(&mut self, info: crate::ImageInfo) -> Result<Self::Image, crate::ErrorKind> {
        Ok(Image {
            texture: Texture::Internal(self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: info.width() as u32,
                    height: info.height() as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: match info.format() {
                    crate::PixelFormat::Rgb8 => wgpu::TextureFormat::Rgba8Unorm,
                    crate::PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
                    crate::PixelFormat::Gray8 => wgpu::TextureFormat::R8Unorm,
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })),
            info,
        })
    }

    fn create_image_from_native_texture(
        &mut self,
        native_texture: Self::NativeTexture,
        info: crate::ImageInfo,
    ) -> Result<Self::Image, crate::ErrorKind> {
        Ok(Image {
            texture: Texture::Internal(native_texture),
            info,
        })
    }

    fn create_image_from_external_texture(
        &mut self,
        external_texture: Self::ExternalTexture,
        info: crate::ImageInfo,
    ) -> Result<Self::Image, crate::ErrorKind> {
        Ok(Image {
            texture: Texture::External(external_texture),
            info,
        })
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: crate::ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), crate::ErrorKind> {
        use rgb::ComponentBytes;

        data.check_update(&image.info, x, y)?;

        let converted_rgba;
        let (bytes, bpp) = match data {
            crate::ImageSource::Rgb(img) => {
                converted_rgba = img
                    .pixels()
                    .map(|rgb| rgb::Rgba {
                        r: rgb.r,
                        g: rgb.g,
                        b: rgb.b,
                        a: 255,
                    })
                    .collect::<Vec<_>>();
                (converted_rgba.as_bytes(), 4)
            }
            crate::ImageSource::Rgba(img) => (img.buf().as_bytes(), 4),
            crate::ImageSource::Gray(img) => (img.buf().as_bytes(), 1),
            #[cfg(target_arch = "wasm32")]
            crate::ImageSource::HtmlImageElement(element) => {
                let size = data.dimensions();
                // WebGPU copies from the natural size, so attribute sizes would overflow or crop the rect.
                let resized =
                    element.width() != element.natural_width() || element.height() != element.natural_height();
                let source = if resized {
                    wgpu::ExternalImageSource::HTMLCanvasElement(rasterize_to_canvas(element, size)?)
                } else {
                    wgpu::ExternalImageSource::HTMLImageElement(element.clone())
                };
                return self.copy_external_image(image, source, size, x, y);
            }
            #[cfg(target_arch = "wasm32")]
            crate::ImageSource::HtmlCanvasElement(element) => {
                let source = wgpu::ExternalImageSource::HTMLCanvasElement(element.clone());
                return self.copy_external_image(image, source, data.dimensions(), x, y);
            }
        };

        if let Texture::Internal(texture) = &image.texture {
            let mut target = texture.as_image_copy();
            target.origin.x = x as _;
            target.origin.y = y as _;

            self.queue.write_texture(
                target,
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpp * data.dimensions().width as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: data.dimensions().width as _,
                    height: data.dimensions().height as _,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image, _image_id: crate::ImageId) {
        if let Texture::Internal(texture) = &image.texture {
            self.stencil_buffer_for_textures.remove(texture);
        }
        drop(image);
    }

    fn screenshot(&mut self) -> Result<imgref::ImgVec<rgb::RGBA8>, crate::ErrorKind> {
        return Err(crate::ErrorKind::UnsupportedOperation);
    }
}

fn gaussian_blur_filter(
    device: &wgpu::Device,
    current_render_target: &mut RenderTarget,
    images: &mut ImageStore<Image>,
    command: super::Command,
    sigma: f32,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    target_image: ImageId,
) {
    let blend_state = blend_state(&command).into();

    let previous_render_target = *current_render_target;

    let source_image = images.get(command.image.unwrap()).unwrap();

    let image_paint = crate::Paint::image(
        command.image.unwrap(),
        0.,
        0.,
        source_image.info.width() as _,
        source_image.info.height() as _,
        0.,
        1.,
    );

    let mut blur_params = Params::new(
        images,
        &Default::default(),
        &image_paint.flavor,
        &Default::default(),
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

    let horizontal_blur_buffer = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("blur horizontal"),
        size: wgpu::Extent3d {
            width: source_image.info.width() as _,
            height: source_image.info.height() as _,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: match source_image.info.format() {
            crate::PixelFormat::Rgb8 => wgpu::TextureFormat::Rgba8Unorm,
            crate::PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            crate::PixelFormat::Gray8 => wgpu::TextureFormat::R8Unorm,
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    render_pass_builder.set_render_target_texture(
        &horizontal_blur_buffer,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::default()),
    );

    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &blur_params,
            images,
            command.image.map(ImageOrTexture::Image),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    render_pass_builder.set_render_target_image(images, target_image, wgpu::LoadOp::Clear(wgpu::Color::default()));

    blur_params.image_blur_filter_direction = [0.0, 1.0];

    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &blur_params,
            images,
            Some(ImageOrTexture::Texture(horizontal_blur_buffer)),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    *current_render_target = previous_render_target;
    match *current_render_target {
        RenderTarget::Screen => {
            render_pass_builder.set_render_target_screen();
        }
        RenderTarget::Image(image_id) => {
            render_pass_builder.set_render_target_image(images, image_id, wgpu::LoadOp::Load);
        }
    }
}

/// Single-pass color-matrix filter: sample the source once and apply the 4x5
/// matrix. Mirrors `gaussian_blur_filter` but without the intermediate texture.
#[allow(clippy::too_many_arguments)]
fn color_matrix_filter(
    current_render_target: &mut RenderTarget,
    images: &mut ImageStore<Image>,
    command: super::Command,
    matrix: [f32; 20],
    render_pass_builder: &mut RenderPassBuilder<'_>,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    target_image: ImageId,
) {
    let blend_state = blend_state(&command).into();
    let previous_render_target = *current_render_target;

    let source_image = images.get(command.image.unwrap()).unwrap();
    let image_paint = crate::Paint::image(
        command.image.unwrap(),
        0.,
        0.,
        source_image.info.width() as _,
        source_image.info.height() as _,
        0.,
        1.,
    );
    let mut params = Params::new(
        images,
        &Default::default(),
        &image_paint.flavor,
        &Default::default(),
        &Scissor::default(),
        0.,
        0.,
        0.,
    );
    params.shader_type = ShaderType::FilterImageColorMatrix;
    // The 4x5 matrix rides the dead scissor/paint-mat slots during the filter
    // pass (see `renderColorMatrix` in the shader) — no uniform-array growth.
    params.scissor_mat.copy_from_slice(&matrix[..12]);
    params.paint_mat[..8].copy_from_slice(&matrix[12..20]);

    render_pass_builder.set_render_target_image(images, target_image, wgpu::LoadOp::Clear(wgpu::Color::default()));

    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &params,
            images,
            command.image.map(ImageOrTexture::Image),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    *current_render_target = previous_render_target;
    match *current_render_target {
        RenderTarget::Screen => {
            render_pass_builder.set_render_target_screen();
        }
        RenderTarget::Image(image_id) => {
            render_pass_builder.set_render_target_image(images, image_id, wgpu::LoadOp::Load);
        }
    }
}

fn triangles(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    params: &Params,
    images: &mut ImageStore<Image>,
) {
    let Some((start, count)) = command.triangles_verts else {
        return;
    };
    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        blend_state(command).into(),
        wgpu::PrimitiveTopology::TriangleList,
        StencilTest::Disabled,
        Some(wgpu::Face::Back),
        params,
        images,
        command.image.map(ImageOrTexture::Image),
        command.glyph_texture,
    );
    render_pass_builder.draw(start as u32..(start + count) as u32);
}

fn stencil_stroke(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    params2: Params,
    images: &mut ImageStore<Image>,
    params1: Params,
) {
    if !command
        .drawables
        .iter()
        .any(|drawable: &super::Drawable| drawable.stroke_verts.is_some())
    {
        return;
    }

    let blend_state = blend_state(command).into();

    // Fill the stroke base without overlap

    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        blend_state,
        wgpu::PrimitiveTopology::TriangleStrip,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::IncrementClamp,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::IncrementClamp,
                },
                read_mask: !0,
                write_mask: !0,
            },
            stencil_reference: 0,
        },
        Some(wgpu::Face::Back),
        &params2,
        images,
        command.image.map(ImageOrTexture::Image),
        command.glyph_texture,
    );

    for drawable in &command.drawables {
        if let Some((start, count)) = drawable.stroke_verts {
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }
    }

    // Draw anti-aliased pixels.

    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        blend_state,
        wgpu::PrimitiveTopology::TriangleStrip,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Keep,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Keep,
                },
                read_mask: !0,
                write_mask: !0,
            },
            stencil_reference: 0,
        },
        Some(wgpu::Face::Back),
        &params1,
        images,
        command.image.map(ImageOrTexture::Image),
        command.glyph_texture,
    );

    for drawable in &command.drawables {
        if let Some((start, count)) = drawable.stroke_verts {
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }
    }

    // clear stencil buffer

    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        None,
        wgpu::PrimitiveTopology::TriangleStrip,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Zero,
                    depth_fail_op: wgpu::StencilOperation::Zero,
                    pass_op: wgpu::StencilOperation::Zero,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Zero,
                    depth_fail_op: wgpu::StencilOperation::Zero,
                    pass_op: wgpu::StencilOperation::Zero,
                },
                read_mask: !0,
                write_mask: !0,
            },
            stencil_reference: 0,
        },
        Some(wgpu::Face::Back),
        &params1,
        images,
        command.image.map(ImageOrTexture::Image),
        command.glyph_texture,
    );

    for drawable in &command.drawables {
        if let Some((start, count)) = drawable.stroke_verts {
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }
    }
}

fn stroke(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    params: Params,
    images: &mut ImageStore<Image>,
) {
    for drawable in &command.drawables {
        let Some((start, count)) = drawable.stroke_verts else {
            continue;
        };
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state(command).into(),
            wgpu::PrimitiveTopology::TriangleStrip,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &params,
            images,
            command.image.map(ImageOrTexture::Image),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

fn concave_fill(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    stencil_params: &Params,
    images: &mut ImageStore<Image>,
    fill_params: &Params,
) {
    if command.drawables.iter().any(|drawable| drawable.fill_verts.is_some()) {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            None,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Enabled {
                stencil_state: wgpu::StencilState {
                    front: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::IncrementWrap,
                    },
                    back: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::DecrementWrap,
                    },
                    read_mask: !0,
                    write_mask: !0,
                },
                stencil_reference: 0,
            },
            None,
            stencil_params,
            images,
            None,
            GlyphTexture::None,
        );

        for drawable in &command.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                render_pass_builder.draw(start as u32..(start + count) as u32);
            }
        }
    }

    let blend_state = blend_state(command);

    if command.drawables.iter().any(|drawable| drawable.stroke_verts.is_some()) {
        for drawable in &command.drawables {
            // draw fringes
            pipeline_and_bindgroup_mapper.update_renderpass(
                render_pass_builder,
                blend_state.into(),
                wgpu::PrimitiveTopology::TriangleStrip,
                StencilTest::Enabled {
                    stencil_state: wgpu::StencilState {
                        front: wgpu::StencilFaceState {
                            compare: wgpu::CompareFunction::Equal,
                            fail_op: wgpu::StencilOperation::Keep,
                            depth_fail_op: wgpu::StencilOperation::Keep,
                            pass_op: wgpu::StencilOperation::Keep,
                        },
                        back: wgpu::StencilFaceState {
                            compare: wgpu::CompareFunction::Equal,
                            fail_op: wgpu::StencilOperation::Keep,
                            depth_fail_op: wgpu::StencilOperation::Keep,
                            pass_op: wgpu::StencilOperation::Keep,
                        },
                        read_mask: match command.fill_rule {
                            FillRule::NonZero => 0xff,
                            FillRule::EvenOdd => 0x1,
                        },
                        write_mask: match command.fill_rule {
                            FillRule::NonZero => 0xff,
                            FillRule::EvenOdd => 0x1,
                        },
                    },
                    stencil_reference: 0,
                },
                Some(wgpu::Face::Back),
                fill_params,
                images,
                command.image.map(ImageOrTexture::Image),
                command.glyph_texture,
            );

            if let Some((start, count)) = drawable.stroke_verts {
                render_pass_builder.draw(start as u32..(start + count) as u32);
            }
        }
    }

    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state.into(),
            wgpu::PrimitiveTopology::TriangleStrip,
            StencilTest::Enabled {
                stencil_state: wgpu::StencilState {
                    front: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::NotEqual,
                        fail_op: wgpu::StencilOperation::Zero,
                        depth_fail_op: wgpu::StencilOperation::Zero,
                        pass_op: wgpu::StencilOperation::Zero,
                    },
                    back: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::NotEqual,
                        fail_op: wgpu::StencilOperation::Zero,
                        depth_fail_op: wgpu::StencilOperation::Zero,
                        pass_op: wgpu::StencilOperation::Zero,
                    },
                    read_mask: match command.fill_rule {
                        FillRule::NonZero => 0xff,
                        FillRule::EvenOdd => 0x1,
                    },
                    write_mask: match command.fill_rule {
                        FillRule::NonZero => 0xff,
                        FillRule::EvenOdd => 0x1,
                    },
                },
                stencil_reference: 0,
            },
            Some(wgpu::Face::Back),
            fill_params,
            images,
            command.image.map(ImageOrTexture::Image),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

fn convex_fill(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    params: &Params,
    images: &mut ImageStore<Image>,
) {
    let blend_state = blend_state(command).into();

    for drawable in &command.drawables {
        if let Some((start, count)) = drawable.fill_verts {
            pipeline_and_bindgroup_mapper.update_renderpass(
                render_pass_builder,
                blend_state,
                wgpu::PrimitiveTopology::TriangleList,
                StencilTest::Disabled,
                Some(wgpu::Face::Back),
                params,
                images,
                command.image.map(ImageOrTexture::Image),
                command.glyph_texture,
            );
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }

        if let Some((start, count)) = drawable.stroke_verts {
            pipeline_and_bindgroup_mapper.update_renderpass(
                render_pass_builder,
                blend_state,
                wgpu::PrimitiveTopology::TriangleStrip,
                StencilTest::Disabled,
                Some(wgpu::Face::Back),
                params,
                images,
                command.image.map(ImageOrTexture::Image),
                command.glyph_texture,
            );
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }
    }
}

fn clear_rect(
    images: &mut ImageStore<Image>,
    color: crate::Color,
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
) {
    let mut params = Params::new(
        images,
        &Default::default(),
        &crate::paint::PaintFlavor::Color(color),
        &Default::default(),
        &Scissor::default(),
        0.,
        0.,
        0.,
    );
    params.shader_type = ShaderType::FillColorUnclipped;
    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled, // ### clear stencil mask
            None,
            &params,
            images,
            None,
            Default::default(),
        );

        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

#[derive(Clone, PartialEq, Debug)]
enum StencilTest {
    Disabled,
    Enabled {
        stencil_state: wgpu::StencilState,
        stencil_reference: u32,
    },
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct PipelineState {
    shader_type: ShaderType,
    enable_glyph_texture: bool,
    render_to_texture: bool,
    color_target_state: wgpu::ColorTargetState,
    primitive_topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    stencil_state: Option<wgpu::StencilState>,
}

impl PipelineState {
    fn new(
        color_blend: Option<wgpu::BlendState>,
        stencil_test: StencilTest,
        format: wgpu::TextureFormat,
        shader_type: ShaderType,
        enable_glyph_texture: bool,
        render_to_texture: bool,
        primitive_topology: wgpu::PrimitiveTopology,
        cull_mode: Option<wgpu::Face>,
        has_stencil_buffer: bool,
    ) -> Self {
        let (stencil_state, color_target_state) = match &stencil_test {
            StencilTest::Enabled { stencil_state, .. } => (
                stencil_state.clone(),
                wgpu::ColorTargetState {
                    format,
                    blend: color_blend,
                    write_mask: if color_blend.is_some() {
                        wgpu::ColorWrites::ALL
                    } else {
                        wgpu::ColorWrites::empty()
                    },
                },
            ),
            StencilTest::Disabled => (
                wgpu::StencilState {
                    front: wgpu::StencilFaceState::IGNORE,
                    back: wgpu::StencilFaceState::IGNORE,
                    read_mask: !0,
                    write_mask: !0,
                },
                wgpu::ColorTargetState {
                    format,
                    blend: color_blend,
                    write_mask: wgpu::ColorWrites::ALL,
                },
            ),
        };
        Self {
            shader_type,
            enable_glyph_texture,
            render_to_texture,
            color_target_state,
            primitive_topology,
            cull_mode,
            stencil_state: has_stencil_buffer.then_some(stencil_state),
        }
    }

    fn materialize(
        &self,
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        shader_module: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        let vertex_entry_point = if self.render_to_texture {
            "vs_main_texture"
        } else {
            "vs_main"
        };

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader_module,
                entry_point: Some(vertex_entry_point),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(self.color_target_state.clone())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: self.primitive_topology,
                front_face: if self.render_to_texture {
                    wgpu::FrontFace::Cw
                } else {
                    wgpu::FrontFace::Ccw
                },
                cull_mode: self.cull_mode,
                ..Default::default()
            },
            depth_stencil: self
                .stencil_state
                .as_ref()
                .map(|stencil_state| wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Stencil8,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: stencil_state.clone(),
                    bias: Default::default(),
                }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

#[derive(Clone, PartialEq)]
enum ImageOrTexture {
    Image(ImageId),
    Texture(wgpu::Texture),
}

#[derive(Clone, PartialEq)]
struct BindGroupState {
    image: Option<ImageOrTexture>,
    glyph_texture: GlyphTexture,
}

impl BindGroupState {
    fn materialize(
        &self,
        device: &wgpu::Device,
        images: &ImageStore<Image>,
        bind_group_layout: &wgpu::BindGroupLayout,
        empty_texture_view: &wgpu::TextureView,
        sampler_cache: &RefCell<HashMap<crate::ImageFlags, wgpu::Sampler>>,
        uniform_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let (main_texture_view, main_sampler) = RenderPassBuilder::create_binding_resource_and_sampler(
            device,
            images,
            self.image.as_ref(),
            empty_texture_view,
            sampler_cache,
        );
        let (glyph_texture_view, glyph_sampler) = RenderPassBuilder::create_binding_resource_and_sampler(
            device,
            images,
            self.glyph_texture.image_id().map(ImageOrTexture::Image).as_ref(),
            empty_texture_view,
            sampler_cache,
        );

        if main_texture_view.is_external() || glyph_texture_view.is_external() {
            unimplemented!("External texture shaders and bind groups are not implemented yet");
        }

        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: (&main_texture_view).into(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&main_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: (&glyph_texture_view).into(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&glyph_sampler),
                },
            ],
            label: None,
        })
    }
}

struct RenderPassBuilder<'a> {
    device: wgpu::Device,
    encoder: &'a mut wgpu::CommandEncoder,
    surface_view: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,
    texture_view: wgpu::TextureView,
    stencil_buffer: Option<wgpu::Texture>,
    viewport: [f32; 2],
    vertex_buffer: wgpu::Buffer,
    rendering_to_texture: bool,
    viewport_bind_group_layout: wgpu::BindGroupLayout,
    rpass: Option<wgpu::RenderPass<'a>>,
    screen_stencil_buffer: wgpu::Texture,
    screen_view: [f32; 2],
    screen_surface_format: wgpu::TextureFormat,
    stencil_buffer_for_textures: &'a mut HashMap<wgpu::Texture, wgpu::Texture>,
    viewport_bind_group: wgpu::BindGroup,
    current_pipeline_state: Option<PipelineState>,
    current_stencil_reference: Option<u32>,
    current_bound_offset: Option<u32>,
}

impl<'a> RenderPassBuilder<'a> {
    fn new(
        device: wgpu::Device,
        encoder: &'a mut wgpu::CommandEncoder,
        screen_surface_format: wgpu::TextureFormat,
        screen_view: [f32; 2],
        viewport_bind_group_layout: wgpu::BindGroupLayout,
        stencil_buffer_for_textures: &'a mut HashMap<wgpu::Texture, wgpu::Texture>,
        texture_view: wgpu::TextureView,
        stencil_buffer: wgpu::Texture,
        vertex_buffer: wgpu::Buffer,
    ) -> Self {
        let viewport_bind_group = Self::create_viewport_bind_group(&device, &screen_view, &viewport_bind_group_layout);
        Self {
            device: device.clone(),
            encoder,
            surface_view: texture_view.clone(),
            surface_format: screen_surface_format,
            texture_view,
            stencil_buffer: Some(stencil_buffer.clone()),
            viewport: screen_view,
            vertex_buffer,
            rendering_to_texture: false,
            viewport_bind_group_layout,
            rpass: None,
            screen_stencil_buffer: stencil_buffer,
            screen_view,
            screen_surface_format,
            stencil_buffer_for_textures,
            viewport_bind_group,
            current_pipeline_state: None,
            current_stencil_reference: None,
            current_bound_offset: None,
        }
    }

    fn set_viewport(&mut self, viewport: [f32; 2]) {
        if self.viewport == viewport {
            return;
        }
        self.viewport = viewport;
        self.viewport_bind_group =
            Self::create_viewport_bind_group(&self.device, &self.viewport, &self.viewport_bind_group_layout);
    }

    fn create_viewport_bind_group(
        device: &wgpu::Device,
        viewport: &[f32; 2],
        viewport_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        // WebGL requires 16 byte alignment for uniforms, so pad accordingly.
        let viewport_padded: [f32; 4] = [viewport[0], viewport[1], 0.0, 0.0];

        let view_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Uniform Buffer for Viewport"),
            contents: bytemuck::cast_slice(&viewport_padded),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: viewport_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buf.as_entire_binding(),
            }],
            label: None,
        });

        viewport_bind_group
    }

    fn create_binding_resource_and_sampler(
        device: &wgpu::Device,
        images: &ImageStore<Image>,
        image: Option<&ImageOrTexture>,
        empty_texture_view: &wgpu::TextureView,
        sampler_cache: &RefCell<HashMap<crate::ImageFlags, wgpu::Sampler>>,
    ) -> (OwnedBindingResource, wgpu::Sampler) {
        let flags = image
            .and_then(|image_or_texture| match image_or_texture {
                ImageOrTexture::Image(image_id) => images.get(*image_id).map(|img| img.info.flags()),
                _ => None,
            })
            .unwrap_or_else(crate::ImageFlags::empty);

        let filter_mode = if flags.contains(crate::ImageFlags::NEAREST) {
            wgpu::FilterMode::Nearest
        } else {
            wgpu::FilterMode::Linear
        };

        let sampler = sampler_cache
            .borrow_mut()
            .entry(flags & SAMPLER_FLAGS)
            .or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    address_mode_u: if flags.contains(crate::ImageFlags::REPEAT_X) {
                        wgpu::AddressMode::Repeat
                    } else {
                        wgpu::AddressMode::ClampToEdge
                    },
                    address_mode_v: if flags.contains(crate::ImageFlags::REPEAT_Y) {
                        wgpu::AddressMode::Repeat
                    } else {
                        wgpu::AddressMode::ClampToEdge
                    },
                    address_mode_w: wgpu::AddressMode::ClampToEdge,
                    mag_filter: filter_mode,
                    min_filter: filter_mode,
                    ..Default::default()
                })
            })
            .clone();

        let binding_resource = image
            .and_then(|image_or_texture| match image_or_texture {
                ImageOrTexture::Image(image_id) => images.get(*image_id).map(|img| match &img.texture {
                    Texture::Internal(texture) => {
                        OwnedBindingResource::TextureView(texture.create_view(&Default::default()))
                    }
                    Texture::External(texture) => OwnedBindingResource::ExternalTexture(texture.clone()),
                }),
                ImageOrTexture::Texture(texture) => Some(OwnedBindingResource::TextureView(
                    texture.create_view(&Default::default()),
                )),
            })
            .unwrap_or_else(|| OwnedBindingResource::TextureView(empty_texture_view.clone()));

        (binding_resource, sampler)
    }

    fn set_render_target_texture(
        &mut self,
        texture: &wgpu::Texture,
        stencil_buffer: Option<wgpu::Texture>,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        self.texture_view = texture.create_view(&Default::default());
        self.set_viewport([texture.width() as f32, texture.height() as f32]);
        self.stencil_buffer = stencil_buffer;
        self.surface_format = texture.format();
        self.rendering_to_texture = true;

        self.recreate_render_pass(load);
    }

    fn set_render_target_image(
        &mut self,
        images: &mut ImageStore<Image>,
        image_id: ImageId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let image = images.get(image_id).unwrap();

        if let Texture::Internal(texture) = &image.texture {
            let stencil_buffer = self
                .stencil_buffer_for_textures
                .entry(texture.clone())
                .or_insert_with(|| {
                    self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Stencil buffer"),
                        size: wgpu::Extent3d {
                            width: texture.width(),
                            height: texture.height(),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Stencil8,
                        view_formats: &[],
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    })
                })
                .clone();

            self.set_render_target_texture(&texture.clone(), Some(stencil_buffer), load);
        }
    }

    fn set_render_target_screen(&mut self) {
        self.texture_view = self.surface_view.clone();
        self.stencil_buffer = Some(self.screen_stencil_buffer.clone());
        self.set_viewport(self.screen_view);
        self.surface_format = self.screen_surface_format;
        self.rendering_to_texture = false;

        self.recreate_render_pass(wgpu::LoadOp::Load);
    }

    fn recreate_render_pass(&mut self, load: wgpu::LoadOp<wgpu::Color>) {
        // A new render pass resets state, so nothing set on the previous one still counts.
        self.current_pipeline_state = None;
        self.current_stencil_reference = None;
        self.current_bound_offset = None;
        drop(self.rpass.take());
        let stencil_view = self
            .stencil_buffer
            .as_ref()
            .map(|buffer| buffer.create_view(&Default::default()));

        let mut rpass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.texture_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: stencil_view
                .as_ref()
                .map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_viewport(0., 0., self.viewport[0], self.viewport[1], 0., 0.);
        rpass.set_bind_group(0, &self.viewport_bind_group, &[]);
        self.rpass = Some(rpass.forget_lifetime());
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>) {
        self.rpass.as_mut().unwrap().draw(vertices, 0..1);
    }
}

struct CommandToPipelineAndBindGroupMapper {
    device: wgpu::Device,
    empty_texture_view: wgpu::TextureView,
    sampler_cache: SamplerCache,
    uniform_buffer: wgpu::Buffer,
    uniform_stride: u64,
    uniform_staging: Vec<u8>,
    current_uniforms: Option<UniformArray>,
    shader_module: Rc<wgpu::ShaderModule>,

    current_bind_group_state: Option<BindGroupState>,
    current_bind_group: Option<wgpu::BindGroup>,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
    pipeline_layout: wgpu::PipelineLayout,
}

impl CommandToPipelineAndBindGroupMapper {
    fn new(
        device: wgpu::Device,
        empty_texture_view: wgpu::TextureView,
        sampler_cache: SamplerCache,
        uniform_buffer: wgpu::Buffer,
        uniform_stride: u64,
        shader_module: Rc<wgpu::ShaderModule>,
        bind_group_layout: wgpu::BindGroupLayout,
        pipeline_layout: wgpu::PipelineLayout,
        pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
    ) -> Self {
        Self {
            device: device.clone(),
            empty_texture_view,
            sampler_cache,
            uniform_buffer,
            uniform_stride,
            uniform_staging: Vec::new(),
            current_uniforms: None,
            shader_module,
            current_bind_group_state: None,
            current_bind_group: None,
            bind_group_layout,
            pipeline_cache,
            pipeline_layout,
        }
    }

    fn update_renderpass<'a>(
        &mut self,
        render_pass_builder: &'a mut RenderPassBuilder<'_>,
        color_blend: Option<wgpu::BlendState>,
        primitive_topology: wgpu::PrimitiveTopology,
        stencil_test: StencilTest,
        cull_mode: Option<wgpu::Face>,
        params: &Params,
        images: &'a ImageStore<Image>,
        image: Option<ImageOrTexture>,
        glyph_texture: GlyphTexture,
    ) {
        let render_pass = render_pass_builder.rpass.as_mut().unwrap();

        let stencil_reference = match &stencil_test {
            StencilTest::Enabled { stencil_reference, .. } => *stencil_reference,
            _ => 0,
        };
        if render_pass_builder.current_stencil_reference != Some(stencil_reference) {
            render_pass.set_stencil_reference(stencil_reference);
            render_pass_builder.current_stencil_reference = Some(stencil_reference);
        }

        let bind_group_state = BindGroupState { image, glyph_texture };

        let bind_group_changed = self.current_bind_group_state != Some(bind_group_state.clone());
        if bind_group_changed {
            self.current_bind_group = bind_group_state
                .materialize(
                    &self.device,
                    images,
                    &self.bind_group_layout,
                    &self.empty_texture_view,
                    &self.sampler_cache,
                    &self.uniform_buffer,
                )
                .into();
            self.current_bind_group_state = Some(bind_group_state);
        }

        let uniforms = UniformArray::from(params);
        if self.current_uniforms.as_ref() != Some(&uniforms) {
            let end = self.uniform_staging.len() + self.uniform_stride as usize;
            self.uniform_staging
                .extend_from_slice(bytemuck::cast_slice(uniforms.as_slice()));
            self.uniform_staging.resize(end, 0);
            self.current_uniforms = Some(uniforms);
        }
        // The current command's slot is the last one staged.
        let offset = (self.uniform_staging.len() - self.uniform_stride as usize) as u32;
        if bind_group_changed || render_pass_builder.current_bound_offset != Some(offset) {
            render_pass.set_bind_group(1, self.current_bind_group.as_ref().unwrap(), &[offset]);
            render_pass_builder.current_bound_offset = Some(offset);
        }

        let pipeline_state = PipelineState::new(
            color_blend,
            stencil_test,
            render_pass_builder.surface_format,
            params.shader_type,
            params.uses_glyph_texture(),
            render_pass_builder.rendering_to_texture,
            primitive_topology,
            cull_mode,
            render_pass_builder.stencil_buffer.is_some(),
        );

        // An unchanged pipeline was looked up, and marked accessed, when it was bound.
        if render_pass_builder.current_pipeline_state.as_ref() != Some(&pipeline_state) {
            let mut pipeline_cache = self.pipeline_cache.borrow_mut();
            let render_pipeline = pipeline_cache.entry(pipeline_state.clone()).or_insert_with(|| {
                let pipeline = pipeline_state.materialize(&self.device, &self.pipeline_layout, &self.shader_module);
                CachedPipeline {
                    pipeline,
                    accessed: false,
                }
            });
            render_pipeline.accessed = true;
            render_pass.set_pipeline(&render_pipeline.pipeline);
            render_pass_builder.current_pipeline_state = Some(pipeline_state);
        }
    }
}

fn blend_state(command: &super::Command) -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: blend_factor(command.composite_operation.src_rgb),
            dst_factor: blend_factor(command.composite_operation.dst_rgb),
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: blend_factor(command.composite_operation.src_alpha),
            dst_factor: blend_factor(command.composite_operation.dst_alpha),
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn blend_factor(factor: BlendFactor) -> wgpu::BlendFactor {
    match factor {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::SrcColor => wgpu::BlendFactor::Src,
        BlendFactor::OneMinusSrcColor => wgpu::BlendFactor::OneMinusSrc,
        BlendFactor::DstColor => wgpu::BlendFactor::Dst,
        BlendFactor::OneMinusDstColor => wgpu::BlendFactor::OneMinusDst,
        BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        BlendFactor::SrcAlphaSaturate => wgpu::BlendFactor::SrcAlphaSaturated,
    }
}
