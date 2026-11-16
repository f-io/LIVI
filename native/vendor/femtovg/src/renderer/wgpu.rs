use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fnv::FnvHashMap;
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
use crate::Transform2D;

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
            view: texture.create_view(&base_level()),
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

use super::FanIndices;
use super::Params;
use super::Vertex;
use crate::clip::{ClipCoverage, MaskCoverage};

const UNIFORMARRAY_SIZE: usize = 16;
const UNIFORM_BYTES: u64 = (UNIFORMARRAY_SIZE * 4 * 4) as u64;
// A concave fill and a stencil stroke record two sets of params, every other command one.
const UNIFORM_SLOTS_PER_COMMAND: u64 = 2;

/// Submits each frame in slices of render passes instead of one command
/// buffer holding every pass; see [`WGPURenderer::set_submission_slicing`].
///
/// wgpu opens a Metal command buffer for every render pass of a
/// `CommandEncoder` and commits them together at submit (gfx-rs/wgpu#5738),
/// and the driver keeps about 2.3 MiB for each, whatever the target size,
/// until it completes. One command buffer per frame therefore holds that
/// much for every pass of the frame at once: a portrait of 200 filtered
/// layers is 969 passes and 2.2 GiB of driver memory on an M4 Max, 1,600
/// opacity layers failed buffer creation, and a 176-layer portrait at
/// 460x260 was jetsammed on an iPhone 12 at the 2.2 GiB per-process limit.
///
/// A slice is submitted before the render pass that would exceed `passes`
/// begins, wherever that pass comes from - a layer, a filter chain, a
/// clear - so each submission holds at most that many, and the driver
/// recycles the memory of the slices that complete while the rest of the
/// frame is still being encoded. Nothing waits for them: the renderer never
/// polls the device, so slicing lowers the peak without bounding it. Over
/// 27 portraits of 87 to 991 passes the sliced frames held between a
/// quarter and all of the unsliced driver memory on the M4 Max, 60 % at the
/// median, and the 969-pass frame held 0.6 GiB; a frame of fewer passes
/// than a slice is submitted as without slicing, in the command buffer the
/// flush returns.
///
/// The slices before the command buffer
/// [`Canvas::flush_to_output`](crate::Canvas::flush_to_output) returns are
/// submitted on the renderer's queue during the flush. Work the caller needs
/// ordered before the frame - a clear, a copy, an upload - must be submitted
/// before the flush is called, since a command buffer submitted together
/// with the returned one runs after the slices and a clear there erases
/// them. Each of those submits, like the caller's own, fires the map and
/// work-done callbacks of work that has already completed.
///
/// So the returned command buffer has to be submitted before the next
/// flush, which would otherwise run its slices first. With slicing on, a
/// frame's vertices, uniforms and image uploads therefore go through the
/// queue, ahead of its first slice, rather than in the frame's command
/// buffers as without slicing. Changing the slicing between a flush and the
/// submit of the command buffer it returned is not supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmissionSlicing {
    /// Render passes per submitted command buffer; at least one.
    pub passes: u32,
}

impl Default for SubmissionSlicing {
    /// 64 passes per slice.
    fn default() -> Self {
        Self { passes: 64 }
    }
}

const MIN_UNIFORM_SLOTS: u64 = 64;
const MIN_VERTEX_BYTES: u64 = 4096;
// What a frame uploads goes through staging chunks of this size, or larger for a larger upload.
const STAGING_CHUNK_BYTES: u64 = 1 << 20;

/// An image upload staged for the next frame's command buffer: where it is
/// copied from and where it goes.
#[derive(Debug)]
struct PendingCopy {
    source: CopySource,
    texture: wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
}

/// Where a staged upload is copied from.
#[derive(Debug)]
enum CopySource {
    /// Bytes in the staging belt, or in a buffer of the upload's own.
    Buffer {
        buffer: wgpu::Buffer,
        offset: u64,
        bytes_per_row: u32,
    },
    /// A browser image source, which the queue copied into a texture of its
    /// own when it was uploaded.
    #[cfg(wasm_unknown)]
    Texture(wgpu::Texture),
}

const UNIFORM_BUFFER_LABEL: &str = "Fragment Uniform Buffer";
const UNIFORM_BUFFER_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::UNIFORM.union(wgpu::BufferUsages::COPY_DST);
const VERTEX_BUFFER_LABEL: &str = "Main Vertex Buffer";
const VERTEX_BUFFER_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::VERTEX.union(wgpu::BufferUsages::COPY_DST);
const FAN_INDEX_LABEL: &str = "femtovg fan indices";
const FAN_INDEX_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::INDEX.union(wgpu::BufferUsages::COPY_DST);

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
        Self([0.0; UNIFORMARRAY_SIZE * 4])
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

    /// The clip shape, read by the shader variant of a draw under one: the
    /// rest of the row the conic angle starts, and the two rows after it.
    pub fn set_clip(&mut self, clip: Option<&ClipCoverage>) {
        if let Some(clip) = clip {
            self.0[54..64].copy_from_slice(&clip.uniform_rows());
        }
    }

    /// The coverage mask, read by the shader variant of a draw under one:
    /// where its corner lies, the pixels it spans and what one of them is
    /// of its image, in the halves of the scissor matrix's columns that no
    /// shader reads, and beside the conic angle whether it takes a pixel
    /// whole or not at all.
    pub fn set_clip_mask(&mut self, mask: Option<&MaskCoverage>) {
        if let Some(mask) = mask {
            self.0[2..4].copy_from_slice(&mask.origin);
            self.0[6..8].copy_from_slice(&mask.size);
            self.0[10..12].copy_from_slice(&mask.texel);
            self.0[53] = f32::from(u8::from(mask.hard));
        }
    }

    /// An image paint's transfer, in the paint matrix's first column's w
    /// (`paint_mat[0].w`), which no image paint reads. Written only for a
    /// paint that has one: a filter pass keeps its own values in that column.
    pub fn set_image_transfer(&mut self, transfer: Option<crate::paint::Transfer>) {
        if let Some(transfer) = transfer {
            self.0[15] = transfer.uniform();
        }
    }

    /// See [`GRADIENT_PREMULTIPLIED_INDEX`](crate::renderer::params::GRADIENT_PREMULTIPLIED_INDEX).
    pub fn set_gradient_premultiplied(&mut self, premultiplied: bool) {
        if premultiplied {
            self.0[crate::renderer::params::GRADIENT_PREMULTIPLIED_INDEX] = 1.0;
        }
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
        arr.set_clip(params.clip.as_ref());
        arr.set_clip_mask(params.clip_mask.as_ref());
        arr.set_image_transfer(params.image_transfer);
        arr.set_gradient_premultiplied(params.gradient_premultiplied);

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
    /// The renderer made the texture. Only its uploads wait for the next
    /// frame's command buffer: a caller's texture takes them at once, as it
    /// takes the caller's own writes, and may be destroyed by the caller.
    owned: bool,
    /// `WGPURenderer::flush_count` when the image was made: an image made
    /// since the last flush is drawn by no frame yet.
    flushes_at_alloc: u64,
}

// Only these flags change a sampler descriptor; the rest would split the cache for nothing.
const SAMPLER_FLAGS: crate::ImageFlags = crate::ImageFlags::REPEAT_X
    .union(crate::ImageFlags::REPEAT_Y)
    .union(crate::ImageFlags::NEAREST)
    .union(crate::ImageFlags::GENERATE_MIPMAPS);

type SamplerCache = Rc<RefCell<HashMap<crate::ImageFlags, wgpu::Sampler>>>;

/// Pipelines kept between flushes. Above this, a flush evicts the least recently used. Large enough to keep the
/// pipelines of views that are not being drawn.
const PIPELINE_CACHE_CAPACITY: usize = 512;

/// Flushes a pipeline stays protected after its last use, even above `PIPELINE_CACHE_CAPACITY`. A frame is several
/// flushes when the app draws layers, so this keeps a whole frame's pipelines, and views that alternate within a
/// few frames.
const PIPELINE_RECENT_FLUSHES: u64 = 64;

#[derive(Debug)]
struct CachedPipeline {
    pipeline: wgpu::RenderPipeline,
    last_used: u64,
}

/// Evicts the entries whose `last_used` flush is oldest until at most `capacity` remain, but none used within the
/// last `recent_flushes` flushes: a frame that binds more states than that keeps them all.
fn evict_least_recently_used<K: Clone + Eq + std::hash::Hash, V>(
    cache: &mut HashMap<K, V>,
    flush: u64,
    capacity: usize,
    recent_flushes: u64,
    last_used: impl Fn(&V) -> u64,
) {
    if cache.len() <= capacity {
        return;
    }
    let excess = cache.len() - capacity;
    let mut unused: Vec<(u64, K)> = cache
        .iter()
        .filter(|(_, value)| flush.saturating_sub(last_used(value)) > recent_flushes)
        .map(|(key, value)| (last_used(value), key.clone()))
        .collect();
    unused.sort_unstable_by_key(|&(stamp, _)| stamp);
    for (_, key) in unused.into_iter().take(excess) {
        cache.remove(&key);
    }
}

#[cfg(test)]
mod pipeline_eviction_tests {
    use super::{evict_least_recently_used, PIPELINE_CACHE_CAPACITY, PIPELINE_RECENT_FLUSHES};
    use std::collections::HashMap;

    /// Up to capacity a flush evicts nothing, one past it evicts the pipeline used longest ago, and a flush never
    /// evicts a pipeline used within the last `PIPELINE_RECENT_FLUSHES` flushes.
    #[test]
    fn pipelines_are_evicted_only_past_capacity_least_recently_used_first() {
        for len in PIPELINE_CACHE_CAPACITY - 1..=PIPELINE_CACHE_CAPACITY + 1 {
            // Pipeline `i` was last bound in flush `i`; the current flush, a lone clear, rebinds only pipeline 0.
            let flush = len as u64;
            let mut cache: HashMap<usize, u64> = (0..len).map(|i| (i, i as u64)).collect();
            cache.insert(0, flush);
            evict_least_recently_used(
                &mut cache,
                flush,
                PIPELINE_CACHE_CAPACITY,
                PIPELINE_RECENT_FLUSHES,
                |&last_used| last_used,
            );
            assert_eq!(cache.len(), len.min(PIPELINE_CACHE_CAPACITY), "pipelines kept of {len}");
            if len > PIPELINE_CACHE_CAPACITY {
                assert!(!cache.contains_key(&1), "pipeline 1, used longest ago, was kept");
            }
        }

        let len = PIPELINE_CACHE_CAPACITY + 1;
        let mut cache: HashMap<usize, u64> = (0..len).map(|i| (i, 1)).collect();
        evict_least_recently_used(
            &mut cache,
            1,
            PIPELINE_CACHE_CAPACITY,
            PIPELINE_RECENT_FLUSHES,
            |&last_used| last_used,
        );
        assert_eq!(cache.len(), len, "a flush binding {len} states evicted some of them");

        // Past capacity, the oldest pipeline is kept while its last use is within PIPELINE_RECENT_FLUSHES flushes,
        // and evicted once it is one flush older than that.
        let flush = PIPELINE_RECENT_FLUSHES + 2;
        let mut cache: HashMap<usize, u64> = (0..len).map(|i| (i, flush - 1)).collect();
        cache.insert(1, flush - PIPELINE_RECENT_FLUSHES);
        evict_least_recently_used(
            &mut cache,
            flush,
            PIPELINE_CACHE_CAPACITY,
            PIPELINE_RECENT_FLUSHES,
            |&last_used| last_used,
        );
        assert_eq!(
            cache.len(),
            len,
            "pipeline 1, used {PIPELINE_RECENT_FLUSHES} flushes ago, was evicted"
        );
        cache.insert(1, flush - PIPELINE_RECENT_FLUSHES - 1);
        evict_least_recently_used(
            &mut cache,
            flush,
            PIPELINE_CACHE_CAPACITY,
            PIPELINE_RECENT_FLUSHES,
            |&last_used| last_used,
        );
        assert!(
            !cache.contains_key(&1),
            "pipeline 1, used {} flushes ago, was kept",
            PIPELINE_RECENT_FLUSHES + 1
        );
    }
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
    fan_indices: FanIndices,
    fan_index_buffer: wgpu::Buffer,
    // Staging for a frame's vertices, fan indices, uniforms and image uploads, copied by the frame's own command
    // buffers.
    staging_belt: wgpu::util::StagingBelt,
    // Image uploads staged for the next frame's command buffer, copied at its start.
    pending_copies: Vec<PendingCopy>,
    screen_stencil: TargetStencil,
    stencil_buffer_for_textures: HashMap<wgpu::Texture, TargetStencil>,
    viewport_bind_groups: ViewportBindGroups,
    /// Without and with a coverage mask.
    draw_layouts: [DrawLayout; 2],
    pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
    slicing: Option<SubmissionSlicing>,
    last_frame_slices: Vec<u32>,
    /// Built on the first upload of an image with `GENERATE_MIPMAPS`.
    mipmaps: Option<MipmapGenerator>,
    flush_count: u64,
    render_target_reader: RenderTargetReader,
}

/// Rasterizes an image element into an offscreen canvas at the given size.
#[cfg(wasm_unknown)]
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
    /// Uploads a browser-side image source into an image's texture, staged
    /// through a texture of its own when the image stages uploads.
    #[cfg(wasm_unknown)]
    fn copy_external_image(
        &mut self,
        image: &Image,
        source: wgpu::ExternalImageSource,
        size: crate::image::Size,
        x: usize,
        y: usize,
    ) -> Result<(), crate::ErrorKind> {
        let Texture::Internal(texture) = &image.texture else {
            return Err(crate::ErrorKind::UnsupportedOperation);
        };
        let extent = wgpu::Extent3d {
            width: size.width as _,
            height: size.height as _,
            depth_or_array_layers: 1,
        };
        if extent.width == 0 || extent.height == 0 {
            return Ok(());
        }
        // An image no frame has drawn yet (`Canvas::create_image` from an
        // element) cannot be overwritten by a frame's staged copies: the copy
        // goes straight into it, after any upload staged before.
        if self.stages_uploads(image) && image.flushes_at_alloc < self.flush_count {
            // Staged as a byte upload is. The queue copies the source now,
            // and copying it straight into the image would run before a frame
            // flushed earlier and submitted later, whose own staged copies
            // would then land over it for good. So the queue copies it into a
            // texture of its own, and the next frame's command buffer copies
            // that into the image after the uploads staged before it.
            let staging = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("femtovg image source"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: texture.format(),
                // copy_external_image_to_texture needs COPY_DST and RENDER_ATTACHMENT.
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.queue.copy_external_image_to_texture(
                &wgpu::CopyExternalImageSourceInfo {
                    source,
                    origin: wgpu::Origin2d::ZERO,
                    flip_y: false,
                },
                wgpu::CopyExternalImageDestInfo {
                    texture: &staging,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                    color_space: wgpu::PredefinedColorSpace::Srgb,
                    premultiplied_alpha: true,
                },
                extent,
            );
            self.pending_copies.push(PendingCopy {
                source: CopySource::Texture(staging),
                texture: texture.clone(),
                origin: [x as u32, y as u32],
                size: [extent.width, extent.height],
            });
            return Ok(());
        }
        self.submit_pending_copies_of(texture);
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
            extent,
        );
        self.generate_mipmaps(texture);
        Ok(())
    }

    /// Creates a new renderer for the device.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let module = wgpu::ShaderModuleDescriptor {
            label: Some("femtovg"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("wgpu/shader.wgsl"),
                    "\n",
                    include_str!("wgpu/filters.wgsl")
                )
                .into(),
            ),
        };
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

        let draw_layouts = [false, true].map(|mask| DrawLayout::new(&device, &viewport_bind_group_layout, mask));

        let fan_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(FAN_INDEX_LABEL),
            size: MIN_VERTEX_BYTES,
            usage: FAN_INDEX_USAGE,
            mapped_at_creation: false,
        });

        Self {
            staging_belt: wgpu::util::StagingBelt::new(device.clone(), STAGING_CHUNK_BYTES),
            pending_copies: Vec::new(),
            device,
            queue,

            shader_module,

            screen_view: [0.0, 0.0],

            empty_texture_view: empty_texture.create_view(&Default::default()),
            sampler_cache: Rc::new(RefCell::new(HashMap::new())),
            uniform_buffer,
            uniform_stride,
            vertex_buffer,
            fan_indices: FanIndices::default(),
            fan_index_buffer,
            screen_stencil: TargetStencil::default(),
            stencil_buffer_for_textures: HashMap::new(),
            viewport_bind_groups: ViewportBindGroups::new(viewport_bind_group_layout),
            draw_layouts,
            pipeline_cache: Default::default(),
            slicing: None,
            last_frame_slices: Vec::new(),
            mipmaps: None,
            flush_count: 0,
            render_target_reader: RenderTargetReader::default(),
        }
    }

    /// Whether an upload to `image` waits for the next frame's command buffer
    /// (see `update_image`): an image the renderer made, without mip levels,
    /// with slicing off.
    fn stages_uploads(&self, image: &Image) -> bool {
        matches!(&image.texture, Texture::Internal(texture) if texture.mip_level_count() == 1)
            && image.owned
            && self.slicing.is_none()
    }

    /// Submits the uploads staged since the last frame if any of them is to
    /// `texture`: the queue copies an upload it is given before the next
    /// frame's command buffer runs, so the ones staged before go first, and do
    /// not land on what it copies.
    fn submit_pending_copies_of(&mut self, texture: &wgpu::Texture) {
        if self.pending_copies.iter().any(|copy| copy.texture == *texture) {
            self.submit_pending_copies();
        }
    }

    /// Submits every upload staged since the last frame on its own.
    fn submit_pending_copies(&mut self) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.encode_pending_copies(&mut encoder);
        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit(Some(encoder.finish()));
    }

    /// Copies the uploads staged since the last frame into their images, in
    /// the order they were made.
    fn encode_pending_copies(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for copy in self.pending_copies.drain(..) {
            let destination = wgpu::TexelCopyTextureInfo {
                texture: &copy.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: copy.origin[0],
                    y: copy.origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            };
            let size = wgpu::Extent3d {
                width: copy.size[0],
                height: copy.size[1],
                depth_or_array_layers: 1,
            };
            match &copy.source {
                CopySource::Buffer {
                    buffer,
                    offset,
                    bytes_per_row,
                } => encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: *offset,
                            bytes_per_row: Some(*bytes_per_row),
                            rows_per_image: None,
                        },
                    },
                    destination,
                    size,
                ),
                #[cfg(wasm_unknown)]
                CopySource::Texture(source) => {
                    encoder.copy_texture_to_texture(source.as_image_copy(), destination, size)
                }
            }
        }
    }

    /// Fills the levels below the base of an image created with
    /// `GENERATE_MIPMAPS`, after each upload as the OpenGL backend does.
    fn generate_mipmaps(&mut self, texture: &wgpu::Texture) {
        if texture.mip_level_count() < 2 || !texture.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            return;
        }
        let (device, queue) = (&self.device, &self.queue);
        self.mipmaps
            .get_or_insert_with(|| MipmapGenerator::new(device))
            .generate(device, queue, texture);
    }
}

/// A view of a texture's base level alone: what a render pass attaches,
/// since an attachment holds one level while a sampled view holds them all.
fn base_level() -> wgpu::TextureViewDescriptor<'static> {
    wgpu::TextureViewDescriptor {
        mip_level_count: Some(1),
        ..Default::default()
    }
}

/// The mip levels an image gets: every halving down to one texel with
/// `GENERATE_MIPMAPS`, the base level alone without.
fn mip_level_count(info: &crate::ImageInfo) -> u32 {
    if info.flags().contains(crate::ImageFlags::GENERATE_MIPMAPS) {
        1 + (info.width().max(info.height()).max(1) as u32).ilog2()
    } else {
        1
    }
}

/// Fills an image's mip levels: each is the level above sampled bilinearly
/// at its own texel centres, the 2x2 box average `glGenerateMipmap`
/// produces, so both backends minify alike. One render pass per level on
/// a pipeline per texture format, built on first use.
#[derive(Debug)]
struct MipmapGenerator {
    module: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl MipmapGenerator {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("wgpu/mipmap.wgsl"));
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("femtovg mipmap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("femtovg mipmap"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("femtovg mipmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            module,
            bind_group_layout,
            pipeline_layout,
            sampler,
            pipelines: HashMap::new(),
        }
    }

    fn generate(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        let Self {
            module,
            bind_group_layout,
            pipeline_layout,
            sampler,
            pipelines,
        } = self;
        let pipeline = pipelines.entry(texture.format()).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("femtovg mipmap"),
                layout: Some(pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_mipmap"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_mipmap"),
                    compilation_options: Default::default(),
                    targets: &[Some(texture.format().into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        });
        let level = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("femtovg mipmap"),
        });
        for target in 1..texture.mip_level_count() {
            let (source, target) = (level(target - 1), level(target));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("femtovg mipmap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

impl WGPURenderer {
    /// Submits each frame in slices of render passes; see
    /// [`SubmissionSlicing`]. `None`, the default, encodes the frame into the
    /// one command buffer that the flush returns.
    pub fn set_submission_slicing(&mut self, slicing: Option<SubmissionSlicing>) {
        // Sliced frames upload through the queue (see `SubmissionSlicing`), so
        // uploads staged before, waiting for the next frame's command buffer,
        // go now: a short sliced frame's one command buffer may be held while
        // the queue takes a later upload to the same image.
        if slicing.is_some() && self.slicing.is_none() && !self.pending_copies.is_empty() {
            self.submit_pending_copies();
        }
        self.slicing = slicing;
    }

    /// The slicing in effect; see [`Self::set_submission_slicing`].
    pub fn submission_slicing(&self) -> Option<SubmissionSlicing> {
        self.slicing
    }

    /// Render passes in each command buffer of the last frame, in submission
    /// order: every entry but the last was submitted during the flush, the
    /// last is the command buffer the flush returned. Empty until a frame
    /// has been flushed.
    pub fn last_frame_slices(&self) -> &[u32] {
        &self.last_frame_slices
    }
}

/// What a blur's first draw reads the render target through, and
/// whether a blur was skipped yet.
#[derive(Debug, Default)]
struct RenderTargetReader {
    /// The screen's pixels a blur reads, at their place on the screen:
    /// a surface texture can be copied, not sampled. Kept at the output's
    /// size, as the stencil buffer is, since a blur drawn every frame copies
    /// every frame.
    screen_copy: Option<wgpu::Texture>,
    // Blurs run every frame, so a skipped one logs once, not per frame.
    warned: bool,
}

impl RenderTargetReader {
    fn warn_skipped(&mut self, reason: &str) {
        if !std::mem::replace(&mut self.warned, true) {
            log::warn!("blur skipped: {reason}");
        }
    }

    /// Copies `src` of the screen `render_pass_builder` draws on into the
    /// screen copy at the same place, ending the open render pass, and
    /// returns a view of the copy.
    fn copy_screen(
        &mut self,
        render_pass_builder: &mut RenderPassBuilder<'_>,
        [x, y, width, height]: [u32; 4],
    ) -> Option<wgpu::TextureView> {
        let surface = render_pass_builder.surface_view.texture().clone();
        if !surface.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            self.warn_skipped("the output texture lacks COPY_SRC usage");
            return None;
        }
        // A view's mip level and array layer can't be queried. The copy
        // reads mip 0, layer 0, so it only trusts an output that spans the
        // whole of a single-layer texture.
        let size = surface.size();
        if surface.depth_or_array_layers() != 1
            || [size.width as f32, size.height as f32] != render_pass_builder.screen_view
        {
            self.warn_skipped("the output isn't mip 0 of a single-layer texture");
            return None;
        }
        // The canvas size can differ from the output's.
        let width = width.min(size.width.saturating_sub(x));
        let height = height.min(size.height.saturating_sub(y));
        if width == 0 || height == 0 {
            return None;
        }
        // Draws through an sRGB view are encoded on write, so the copy
        // decodes on read: drawing it back then encodes once.
        let format = if render_pass_builder.screen_surface_format.is_srgb() {
            surface.format().add_srgb_suffix()
        } else {
            surface.format().remove_srgb_suffix()
        };
        if self
            .screen_copy
            .as_ref()
            .is_some_and(|copy| copy.format() != format || copy.size() != size)
        {
            self.screen_copy = None;
        }
        let device = &render_pass_builder.device;
        let copy = self.screen_copy.get_or_insert_with(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("femtovg screen copy"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let origin = wgpu::Origin3d { x, y, z: 0 };
        render_pass_builder.end_render_pass().copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &surface,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: copy,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Some(copy.create_view(&Default::default()))
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
        self.flush_count += 1;

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
        // Unsliced, the copies come first in the frame's own command buffer, so a frame flushed before the one
        // before it is submitted cannot overwrite what that one draws with. Sliced, the frame is submitted during
        // the flush and uploads through the queue, ahead of its first slice (see `SubmissionSlicing`).
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.encode_pending_copies(&mut encoder);
        if self.slicing.is_some() {
            if !vertex_bytes.is_empty() {
                self.queue.write_buffer(&self.vertex_buffer, 0, vertex_bytes);
            }
        } else if let Some(size) = wgpu::BufferSize::new(vertex_bytes.len() as u64) {
            self.staging_belt
                .write_buffer(&mut encoder, &self.vertex_buffer, 0, size)
                .copy_from_slice(vertex_bytes);
        }
        let vertex_buffer = self.vertex_buffer.clone();
        self.fan_indices.build(&commands);
        let fan_index_bytes: &[u8] = bytemuck::cast_slice(self.fan_indices.indices());
        grow_buffer(
            &self.device,
            &mut self.fan_index_buffer,
            fan_index_bytes.len() as u64,
            FAN_INDEX_LABEL,
            FAN_INDEX_USAGE,
        );
        if self.slicing.is_some() {
            if !fan_index_bytes.is_empty() {
                self.queue.write_buffer(&self.fan_index_buffer, 0, fan_index_bytes);
            }
        } else if let Some(size) = wgpu::BufferSize::new(fan_index_bytes.len() as u64) {
            self.staging_belt
                .write_buffer(&mut encoder, &self.fan_index_buffer, 0, size)
                .copy_from_slice(fan_index_bytes);
        }

        // A screen of another size starts over: its clip plane went with the old stencil.
        if self
            .screen_stencil
            .texture
            .as_ref()
            .is_some_and(|stencil| stencil.width() != output.width || stencil.height() != output.height)
        {
            self.screen_stencil = TargetStencil::default();
        }
        let stencil_uses = commands.iter().map(StencilUse::of).collect();

        // Unsliced, the frame's one command buffer copies the uniform slots first, written as the commands are
        // recorded; the slots past the last one used are copied too, and never read. Sliced, each slice's slots
        // are written on the queue ahead of its submit.
        let uniforms = match self.slicing {
            None => FrameUniforms::Mapped {
                slots: self.staging_belt.write_buffer(
                    &mut encoder,
                    &self.uniform_buffer,
                    0,
                    wgpu::BufferSize::new(needed_slots * self.uniform_stride).expect("a frame has commands"),
                ),
                bytes: 0,
            },
            Some(_) => FrameUniforms::Staged {
                staging: Vec::new(),
                written: 0,
            },
        };
        let frame = FrameEncoder {
            device: self.device.clone(),
            queue: self.queue.clone(),
            encoder,
            staging_belt: &mut self.staging_belt,
            uniform_buffer: self.uniform_buffer.clone(),
            uniform_stride: self.uniform_stride,
            uniforms,
            slicing: self.slicing,
            slices: Vec::new(),
            passes: 0,
        };
        let mut render_pass_builder = RenderPassBuilder::new(
            self.device.clone(),
            frame,
            output.format,
            self.screen_view,
            &mut self.viewport_bind_groups,
            self.flush_count,
            &mut self.screen_stencil,
            &mut self.stencil_buffer_for_textures,
            stencil_uses,
            texture_view,
            vertex_buffer,
            self.fan_index_buffer.clone(),
        );
        // The screen is the target until a command sets another.
        render_pass_builder.set_render_target_screen();

        let mut pipeline_and_bindgroup_mapper = CommandToPipelineAndBindGroupMapper::new(
            self.device.clone(),
            self.empty_texture_view.clone(),
            self.sampler_cache.clone(),
            self.uniform_buffer.clone(),
            self.shader_module.clone(),
            self.draw_layouts.clone(),
            self.pipeline_cache.clone(),
            self.flush_count,
        );

        let mut current_render_target = RenderTarget::Screen;
        // Whether the last `StartBlur` drew.
        let mut sampled = false;
        for (index, (command, &fans)) in commands.into_iter().zip(&self.fan_indices.firsts).enumerate() {
            render_pass_builder.command_index = index;
            render_pass_builder.set_clip_bounds(&command);
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
                super::CommandType::ClearRect { color, keep_clip } => {
                    clear_rect(
                        images,
                        color,
                        &command,
                        keep_clip,
                        verts,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                    );
                }
                super::CommandType::ConvexFill { ref params } => {
                    convex_fill(
                        &command,
                        fans,
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
                        fans,
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
                super::CommandType::ClipFill => {
                    clip_fill(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        images,
                    );
                }
                super::CommandType::ClipReset { visible } => {
                    clip_reset(
                        &command,
                        &mut pipeline_and_bindgroup_mapper,
                        &mut render_pass_builder,
                        images,
                        visible,
                    );
                }
                super::CommandType::RenderFilteredImage { target_image, filter } => match filter {
                    crate::ImageFilter::GaussianBlur { sigma_x, sigma_y } => {
                        let mut pass = FilterPass {
                            images,
                            current_render_target: &mut current_render_target,
                            render_pass_builder: &mut render_pass_builder,
                            pipeline_and_bindgroup_mapper: &mut pipeline_and_bindgroup_mapper,
                        };
                        gaussian_blur_filter(&mut pass, command, [sigma_x, sigma_y], target_image);
                    }
                    crate::ImageFilter::Morphology {
                        radius_x,
                        radius_y,
                        operator,
                    } => {
                        let mut pass = FilterPass {
                            images,
                            current_render_target: &mut current_render_target,
                            render_pass_builder: &mut render_pass_builder,
                            pipeline_and_bindgroup_mapper: &mut pipeline_and_bindgroup_mapper,
                        };
                        morphology_filter(&mut pass, command, [radius_x, radius_y], operator, target_image);
                    }
                    single_pass => {
                        let target_info = images.get(target_image).unwrap().info;
                        let (shader_type, slots) = single_pass
                            .single_pass(target_info.width() as f32, target_info.height() as f32)
                            .expect("every filter but the Gaussian blur and the morphology runs as one pass");
                        let mut pass = FilterPass {
                            images,
                            current_render_target: &mut current_render_target,
                            render_pass_builder: &mut render_pass_builder,
                            pipeline_and_bindgroup_mapper: &mut pipeline_and_bindgroup_mapper,
                        };
                        single_pass_filter(&mut pass, command, shader_type, slots, target_image);
                    }
                },
                super::CommandType::StartBlur {
                    target_image,
                    src,
                    sigma,
                    sample_transform,
                } => {
                    let mut pass = FilterPass {
                        images,
                        current_render_target: &mut current_render_target,
                        render_pass_builder: &mut render_pass_builder,
                        pipeline_and_bindgroup_mapper: &mut pipeline_and_bindgroup_mapper,
                    };
                    sampled = start_blur(
                        &mut pass,
                        &command,
                        &mut self.render_target_reader,
                        target_image,
                        src,
                        sigma,
                        sample_transform,
                    );
                }
                super::CommandType::FinishBlur { stencil_params, .. } => {
                    // Its source is the first draw's image, or that image run
                    // down a pyramid: without that draw it would write back
                    // whatever the image held before.
                    if !sampled {
                        continue;
                    }
                    // The fills take their blend from the command.
                    let mut command = command;
                    for (composite_operation, params) in command.finish_blur_draws() {
                        command.composite_operation = composite_operation;
                        match &stencil_params {
                            Some(stencil_params) => concave_fill(
                                &command,
                                fans,
                                &mut pipeline_and_bindgroup_mapper,
                                &mut render_pass_builder,
                                stencil_params,
                                images,
                                &params,
                            ),
                            None => convex_fill(
                                &command,
                                fans,
                                &mut pipeline_and_bindgroup_mapper,
                                &mut render_pass_builder,
                                &params,
                                images,
                            ),
                        }
                    }
                }
            }
        }

        let (command_buffer, slices) = render_pass_builder.finish();
        self.last_frame_slices = slices;

        self.viewport_bind_groups.evict(self.flush_count);
        evict_least_recently_used(
            &mut self.pipeline_cache.borrow_mut(),
            self.flush_count,
            PIPELINE_CACHE_CAPACITY,
            PIPELINE_RECENT_FLUSHES,
            |cached_pipeline| cached_pipeline.last_used,
        );

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
                mip_level_count: mip_level_count(&info),
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
            owned: true,
            flushes_at_alloc: self.flush_count,
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
            owned: false,
            flushes_at_alloc: self.flush_count,
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
            owned: false,
            flushes_at_alloc: self.flush_count,
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
            #[cfg(wasm_unknown)]
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
            #[cfg(wasm_unknown)]
            crate::ImageSource::HtmlCanvasElement(element) => {
                let source = wgpu::ExternalImageSource::HTMLCanvasElement(element.clone());
                return self.copy_external_image(image, source, data.dimensions(), x, y);
            }
        };

        if let Texture::Internal(texture) = &image.texture {
            // An image without mip levels is copied at the start of the next frame's own command buffer,
            // every upload in the order it was made, so that a frame draws the image as it was when the frame
            // was flushed: from staging the belt reuses, or, for an upload larger than a chunk, from a buffer of
            // its own, which the belt would keep once recalled. write_texture would copy an upload before every
            // one staged before it. An image with mip levels keeps write_texture: the levels are generated
            // from it at once; so does a caller's texture (`create_image_from_native_texture`), which takes the
            // caller's own writes through the queue and may be destroyed by the caller. With slicing on, frames
            // are submitted during their flush and upload through the queue, as they did before (see
            // `SubmissionSlicing`).
            let size = data.dimensions();
            if self.stages_uploads(image) {
                let row = bpp as u64 * size.width as u64;
                let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
                let padded = row.div_ceil(align) * align;
                let total = padded * size.height as u64;
                let Some(bytes_needed) = wgpu::BufferSize::new(total) else {
                    return Ok(());
                };
                let write = |mut view: wgpu::BufferViewMut| {
                    let (row, padded) = (row as usize, padded as usize);
                    for (line, source) in bytes.chunks_exact(row).take(size.height).enumerate() {
                        view.slice(line * padded..line * padded + row).copy_from_slice(source);
                    }
                };
                let staged = (total <= STAGING_CHUNK_BYTES)
                    .then(|| {
                        let slice = self
                            .staging_belt
                            .allocate(bytes_needed, wgpu::BufferSize::new(align).expect("not zero"));
                        let view = slice.get_mapped_range_mut().ok()?;
                        write(view);
                        Some((slice.buffer().clone(), slice.offset()))
                    })
                    .flatten();
                let (buffer, offset) = match staged {
                    Some(staged) => staged,
                    None => {
                        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("femtovg image upload"),
                            size: total,
                            usage: wgpu::BufferUsages::COPY_SRC,
                            mapped_at_creation: true,
                        });
                        let view = buffer.get_mapped_range_mut(..).map_err(|error| {
                            crate::ErrorKind::GeneralError(format!("mapping an image upload: {error}"))
                        })?;
                        write(view);
                        buffer.unmap();
                        (buffer, 0)
                    }
                };
                self.pending_copies.push(PendingCopy {
                    source: CopySource::Buffer {
                        buffer,
                        offset,
                        bytes_per_row: padded as u32,
                    },
                    texture: texture.clone(),
                    origin: [x as u32, y as u32],
                    size: [size.width as u32, size.height as u32],
                });
                return Ok(());
            }
            self.submit_pending_copies_of(texture);
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
            self.generate_mipmaps(texture);
        }
        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image, _image_id: crate::ImageId) {
        if let Texture::Internal(texture) = &image.texture {
            self.stencil_buffer_for_textures.remove(texture);
        }
        drop(image);
    }

    fn max_texture_size(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    fn transient_image_cost(&self, info: crate::ImageInfo) -> usize {
        wgpu_transient_image_cost(info)
    }

    fn screenshot(&mut self) -> Result<imgref::ImgVec<rgb::RGBA8>, crate::ErrorKind> {
        return Err(crate::ErrorKind::UnsupportedOperation);
    }
}

fn wgpu_transient_image_cost(info: crate::ImageInfo) -> usize {
    let color_bytes = match info.format() {
        crate::PixelFormat::Gray8 => 1,
        crate::PixelFormat::Rgb8 | crate::PixelFormat::Rgba8 => 4,
    };
    // Color plus the target's Stencil8 attachment. Gaussian blur scratch is
    // a separate transient image and is charged independently.
    info.width()
        .saturating_mul(info.height())
        .saturating_mul(color_bytes + 1)
}

#[cfg(test)]
mod transient_cost_tests {
    use super::wgpu_transient_image_cost;
    use crate::{ImageFlags, ImageInfo, PixelFormat};

    #[test]
    fn charges_color_and_stencil() {
        let pixels = 1242 * 2688;
        let rgba = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Rgba8);
        let rgb = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Rgb8);
        let gray = ImageInfo::new(ImageFlags::empty(), 1242, 2688, PixelFormat::Gray8);

        assert_eq!(wgpu_transient_image_cost(rgba), pixels * 5);
        assert_eq!(wgpu_transient_image_cost(rgb), pixels * 5);
        assert_eq!(wgpu_transient_image_cost(gray), pixels * 2);
    }
}

/// The render-loop state an image-filter pass draws through: the images,
/// the target it must restore when done, the open pass builder and the
/// pipeline mapper.
/// The frame's command encoding: the encoder, the uniform slots staged for
/// it, and, with slicing on, the command buffers submitted so far.
struct FrameEncoder<'a> {
    device: wgpu::Device,
    queue: wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    /// The belt the frame's uploads are staged in, finished before each of
    /// the frame's command buffers is: their copies read staging it maps.
    staging_belt: &'a mut wgpu::util::StagingBelt,
    uniform_buffer: wgpu::Buffer,
    uniform_stride: u64,
    uniforms: FrameUniforms,
    slicing: Option<SubmissionSlicing>,
    /// Render passes in each command buffer submitted so far.
    slices: Vec<u32>,
    /// Render passes begun on the current encoder.
    passes: u32,
}

/// Where a frame stages its uniform slots.
enum FrameUniforms {
    /// Unsliced: in the belt, whose copy into the uniform buffer the frame's
    /// one command buffer records first; the bytes of it used so far.
    Mapped { slots: wgpu::BufferViewMut, bytes: usize },
    /// Sliced: in memory, each slice's uploaded ahead of its submit, and how
    /// many bytes have been.
    Staged { staging: Vec<u8>, written: usize },
}

impl FrameEncoder<'_> {
    /// Counts a render pass about to begin on the encoder, which must hold
    /// no open pass; with slicing on, an encoder that already holds the
    /// configured passes is submitted first.
    fn begin_pass(&mut self) {
        if let Some(slicing) = self.slicing {
            if self.passes >= slicing.passes.max(1) {
                self.submit_slice();
            }
        }
        self.passes += 1;
    }

    fn submit_slice(&mut self) {
        let encoder = std::mem::replace(
            &mut self.encoder,
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default()),
        );
        self.write_uniforms();
        // The first slice copies the image uploads staged while slicing was
        // off from the belt; its chunks come back once the slice has run.
        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit(Some(encoder.finish()));
        self.slices.push(self.passes);
        self.passes = 0;
    }

    /// Stages one uniform slot.
    fn stage_uniforms(&mut self, uniforms: &[u8]) {
        let stride = self.uniform_stride as usize;
        match &mut self.uniforms {
            FrameUniforms::Mapped { slots, bytes } => {
                let start = *bytes;
                *bytes += stride;
                debug_assert!(
                    *bytes <= slots.len(),
                    "a command recorded more than UNIFORM_SLOTS_PER_COMMAND uniform slots"
                );
                slots.slice(start..start + uniforms.len()).copy_from_slice(uniforms);
            }
            FrameUniforms::Staged { staging, .. } => {
                let end = staging.len() + stride;
                staging.extend_from_slice(uniforms);
                staging.resize(end, 0);
                debug_assert!(
                    staging.len() as u64 <= self.uniform_buffer.size(),
                    "a command recorded more than UNIFORM_SLOTS_PER_COMMAND uniform slots"
                );
            }
        }
    }

    /// The dynamic offset of the last staged slot.
    fn current_uniform_offset(&self) -> u32 {
        let used = match &self.uniforms {
            FrameUniforms::Mapped { bytes, .. } => *bytes,
            FrameUniforms::Staged { staging, .. } => staging.len(),
        };
        (used - self.uniform_stride as usize) as u32
    }

    /// Uploads the slots staged since the previous upload, ordered ahead of
    /// the next submit; the frame's command buffer copies mapped ones itself.
    fn write_uniforms(&mut self) {
        if let FrameUniforms::Staged { staging, written } = &mut self.uniforms {
            let pending = &staging[*written..];
            if !pending.is_empty() {
                self.queue.write_buffer(&self.uniform_buffer, *written as u64, pending);
                *written = staging.len();
            }
        }
    }

    /// Finishes the frame's last command buffer, and lists the passes of
    /// every command buffer, that one last.
    fn finish(mut self) -> (wgpu::CommandBuffer, Vec<u32>) {
        self.write_uniforms();
        // The view goes before the belt unmaps its chunk, which comes back for
        // reuse once the command buffer has been submitted and run.
        self.uniforms = FrameUniforms::Staged {
            staging: Vec::new(),
            written: 0,
        };
        self.staging_belt.finish_and_recall_on_submit(&self.encoder);
        self.slices.push(self.passes);
        (self.encoder.finish(), self.slices)
    }
}

struct FilterPass<'a, 'b> {
    images: &'a mut ImageStore<Image>,
    current_render_target: &'a mut RenderTarget,
    render_pass_builder: &'a mut RenderPassBuilder<'b>,
    pipeline_and_bindgroup_mapper: &'a mut CommandToPipelineAndBindGroupMapper,
}

impl FilterPass<'_, '_> {
    /// Points the pass builder back at `target`, the render target that was
    /// current before the filter drew into its own, and records it.
    fn restore_target(&mut self, target: RenderTarget) {
        *self.current_render_target = target;
        match target {
            RenderTarget::Screen => {
                self.render_pass_builder.set_render_target_screen();
            }
            RenderTarget::Image(image_id) => {
                self.render_pass_builder
                    .set_render_target_image(self.images, image_id, wgpu::LoadOp::Load);
            }
        }
    }
}

/// A filter of two draws along the axes: the horizontal one, with the
/// parameters `axis_params(params, 0)` sets, into the command's reserved
/// transient scratch, then the vertical one, with `axis_params(params, 1)`,
/// from the scratch into `target_image`.
fn separable_filter(
    pass: &mut FilterPass<'_, '_>,
    command: super::Command,
    target_image: ImageId,
    mut axis_params: impl FnMut(&mut Params, usize),
) {
    let FilterPass {
        images,
        current_render_target,
        render_pass_builder,
        pipeline_and_bindgroup_mapper,
    } = pass;
    let images: &mut ImageStore<Image> = images;
    let current_render_target: &mut RenderTarget = current_render_target;
    let render_pass_builder: &mut RenderPassBuilder<'_> = render_pass_builder;
    let pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper = pipeline_and_bindgroup_mapper;
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
    axis_params(&mut params, 0);
    params.image_blur_filter_direction = [1.0, 0.0];
    // The fused matrices ride parameters a filter draw leaves unused: an
    // alpha-only matrix before the filter is the `radius` flag on the first
    // draw, a matrix after it the `feather` flag and the matrix slots on
    // the second.
    params.radius = f32::from(u8::from(command.fused.source_alpha));

    let horizontal_buffer = command
        .filter_scratch
        .expect("a two-pass filter has a reserved scratch image");
    render_pass_builder.set_filter_target_image(images, horizontal_buffer, wgpu::LoadOp::Clear(wgpu::Color::default()));

    if let Some((start, count)) = command.triangles_verts {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &params,
            images,
            command.image,
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    render_pass_builder.set_filter_target_image(images, target_image, wgpu::LoadOp::Clear(wgpu::Color::default()));

    axis_params(&mut params, 1);
    params.image_blur_filter_direction = [0.0, 1.0];
    // The horizontal draw stored premultiplied RGBA regardless of the source
    // image's format or premultiplication flag.
    params.tex_type = 0.0;
    params.radius = 0.0;
    if let Some(matrix) = command.fused.post_matrix {
        params.scissor_mat.copy_from_slice(&matrix[..12]);
        params.paint_mat[..8].copy_from_slice(&matrix[12..20]);
        params.feather = 1.0;
    }

    if let Some((start, count)) = command
        .triangles_verts
        .filter(|_| render_pass_builder.crop(command.crop))
    {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &params,
            images,
            Some(horizontal_buffer),
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    pass.restore_target(previous_render_target);
}

/// Draws `src` of the render target the pass draws on over `target_image`:
/// see `CommandType::StartBlur`. Returns whether it drew.
fn start_blur(
    pass: &mut FilterPass<'_, '_>,
    command: &super::Command,
    reader: &mut RenderTargetReader,
    target_image: ImageId,
    src: [u32; 4],
    sigma: f32,
    sample_transform: Transform2D,
) -> bool {
    // The stencil plan ends the pass here (`StencilUse::PassEnd`), so the
    // pass ends whether the blur goes ahead or is skipped: a skipped blur
    // that left it open would have the draws after it run in a pass planned
    // only up to here, without the stencil they test or write.
    pass.render_pass_builder.end_render_pass();
    if pass.images.get(target_image).is_none() {
        return false;
    }
    let previous_render_target = *pass.current_render_target;
    // An image target is read where it is; the screen through a copy.
    let (source, size, flip_y) = match previous_render_target {
        RenderTarget::Image(id) => {
            let Some(Texture::Internal(texture)) = pass.images.get(id).map(|image| &image.texture) else {
                return false;
            };
            if !texture.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING) {
                reader.warn_skipped("the render target image can't be sampled");
                return false;
            }
            let size = [texture.width() as f32, texture.height() as f32];
            // A view, unlike the image, takes the default sampler, whatever
            // the image's flags: the taps clamp and the first halving
            // averages only through a linear, clamped lookup of the base level.
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                mip_level_count: Some(1),
                ..Default::default()
            });
            // An image target holds its rows bottom-up.
            (MainTexture::View(view), size, true)
        }
        RenderTarget::Screen => {
            let Some(view) = reader.copy_screen(pass.render_pass_builder, src) else {
                return false;
            };
            let size = view.texture().size();
            (MainTexture::View(view), [size.width as f32, size.height as f32], false)
        }
    };
    let sample_transform = if flip_y {
        sample_transform * Transform2D([1.0, 0.0, 0.0, -1.0, 0.0, size[1]])
    } else {
        sample_transform
    };
    let params = Params::blur_axis(size, sigma, [1.0, 0.0], &sample_transform);

    pass.render_pass_builder.set_filter_target_image(
        pass.images,
        target_image,
        wgpu::LoadOp::Clear(wgpu::Color::default()),
    );
    if let Some((start, count)) = command.triangles_verts {
        pass.pipeline_and_bindgroup_mapper.update_renderpass_sampling(
            pass.render_pass_builder,
            blend_state(command).into(),
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            None,
            &params,
            pass.images,
            source,
            GlyphTexture::None,
        );
        pass.render_pass_builder.draw(start as u32..(start + count) as u32);
    }
    pass.restore_target(previous_render_target);
    true
}

/// Two-pass Gaussian blur of `command.image` into `target_image`: horizontal
/// by `sigma[0]`, then vertical by `sigma[1]`. A degenerate sigma makes that
/// draw a copy.
fn gaussian_blur_filter(
    pass: &mut FilterPass<'_, '_>,
    command: super::Command,
    sigma: [f32; 2],
    target_image: ImageId,
) {
    separable_filter(pass, command, target_image, |params, axis| {
        params.shader_type = ShaderType::FilterImage;
        let (coeff, sigma) = crate::renderer::gaussian_blur_coefficients(sigma[axis]);
        params.image_blur_filter_coeff[..3].copy_from_slice(&coeff);
        params.image_blur_filter_sigma = sigma;
    });
}

/// Two-pass morphology of `command.image` into `target_image`: the
/// per-channel maximum (dilate) or minimum (erode) over `radius[0]` whole
/// pixels either side along x, then over `radius[1]` along y. The radius
/// rides the blur's sigma parameter and the operator its first coefficient.
fn morphology_filter(
    pass: &mut FilterPass<'_, '_>,
    command: super::Command,
    radius: [f32; 2],
    operator: crate::MorphologyOperator,
    target_image: ImageId,
) {
    separable_filter(pass, command, target_image, |params, axis| {
        params.shader_type = ShaderType::FilterImageMorphology;
        params.image_blur_filter_sigma = radius[axis];
        params.image_blur_filter_coeff[0] = f32::from(u8::from(operator == crate::MorphologyOperator::Dilate));
    });
}

/// Single-pass color-matrix filter: sample the source once and apply the 4x5
/// matrix. Mirrors `gaussian_blur_filter` but without the intermediate texture.
/// Runs a one-pass filter (color matrix, turbulence, transfer) over the
/// command's quad into `target_image`, sampling the command's image.
fn single_pass_filter(
    pass: &mut FilterPass<'_, '_>,
    command: super::Command,
    shader_type: ShaderType,
    slots: [f32; 20],
    target_image: ImageId,
) {
    let FilterPass {
        images,
        current_render_target,
        render_pass_builder,
        pipeline_and_bindgroup_mapper,
    } = pass;
    let images: &mut ImageStore<Image> = images;
    let current_render_target: &mut RenderTarget = current_render_target;
    let render_pass_builder: &mut RenderPassBuilder<'_> = render_pass_builder;
    let pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper = pipeline_and_bindgroup_mapper;
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
    // A blend pass binds its backdrop in the glyph-texture slot; every other
    // pass binds nothing there.
    let mut params = Params::new(
        images,
        &Default::default(),
        &image_paint.flavor,
        &command.glyph_texture,
        &Scissor::default(),
        0.,
        0.,
        0.,
    );
    let target_info = images.get(target_image).unwrap().info;
    params.shader_type = shader_type;
    let mut slots = slots;
    if shader_type == ShaderType::FilterImageBlend {
        let pass = command.blend_pass;
        slots[1] = f32::from(u8::from(pass.backdrop_flipped));
        slots[2] = pass.source_alpha;
        slots[3] = f32::from(u8::from(pass.contribution));
    }
    // The filter's parameters ride the dead scissor/paint-mat slots during the
    // pass (see `ImageFilter::single_pass`) — no uniform-array growth.
    params.scissor_mat.copy_from_slice(&slots[..12]);
    params.paint_mat[..8].copy_from_slice(&slots[12..20]);
    // A generating pass binds its lookup table as the image, so the output
    // extent comes from the target rather than from what is sampled.
    params.extent = [target_info.width() as f32, target_info.height() as f32];

    render_pass_builder.set_filter_target_image(images, target_image, wgpu::LoadOp::Clear(wgpu::Color::default()));

    if let Some((start, count)) = command
        .triangles_verts
        .filter(|_| render_pass_builder.crop(command.crop))
    {
        pipeline_and_bindgroup_mapper.update_renderpass(
            render_pass_builder,
            blend_state,
            wgpu::PrimitiveTopology::TriangleList,
            StencilTest::Disabled,
            Some(wgpu::Face::Back),
            &params,
            images,
            command.image,
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }

    pass.restore_target(previous_render_target);
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
        clip_guard(command.clip_active),
        Some(wgpu::Face::Back),
        params,
        images,
        command.image,
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
                // With a clip armed the untouched in-clip value is 0x80 and
                // the stroke counter stays inside the winding bits.
                write_mask: if command.clip_active { 0x7f } else { !0 },
            },
            stencil_reference: if command.clip_active { 0x80 } else { 0 },
        },
        Some(wgpu::Face::Back),
        &params2,
        images,
        command.image,
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
            stencil_reference: if command.clip_active { 0x80 } else { 0 },
        },
        Some(wgpu::Face::Back),
        &params1,
        images,
        command.image,
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
                // Clear the stroke counters; the clip bit is write-protected.
                write_mask: if command.clip_active { 0x7f } else { !0 },
            },
            stencil_reference: 0,
        },
        Some(wgpu::Face::Back),
        &params1,
        images,
        command.image,
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
            clip_guard(command.clip_active),
            Some(wgpu::Face::Back),
            &params,
            images,
            command.image,
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

fn concave_fill(
    command: &super::Command,
    fans: u32,
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
                        // With a clip armed, winding accumulates only inside
                        // the clip and bit 7 stays write-protected.
                        compare: if command.clip_active {
                            wgpu::CompareFunction::Equal
                        } else {
                            wgpu::CompareFunction::Always
                        },
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::IncrementWrap,
                    },
                    back: wgpu::StencilFaceState {
                        compare: if command.clip_active {
                            wgpu::CompareFunction::Equal
                        } else {
                            wgpu::CompareFunction::Always
                        },
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::DecrementWrap,
                    },
                    read_mask: if command.clip_active { 0x80 } else { !0 },
                    write_mask: if command.clip_active { 0x7f } else { !0 },
                },
                stencil_reference: if command.clip_active { 0x80 } else { 0 },
            },
            None,
            stencil_params,
            images,
            None,
            GlyphTexture::None,
        );

        let mut first = fans;
        for drawable in &command.drawables {
            if let Some((_, count)) = drawable.fill_verts {
                render_pass_builder.draw_fan(&mut first, count);
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
                        read_mask: match (command.clip_active, command.fill_rule) {
                            (false, FillRule::NonZero) => 0xff,
                            (false, FillRule::EvenOdd) => 0x1,
                            // Fringe where the clip bit is set and no winding
                            // arrived (stencil == exactly 0x80).
                            (true, FillRule::NonZero) => 0xff,
                            (true, FillRule::EvenOdd) => 0x81,
                        },
                        write_mask: match command.fill_rule {
                            FillRule::NonZero => 0xff,
                            FillRule::EvenOdd => 0x1,
                        },
                    },
                    stencil_reference: if command.clip_active { 0x80 } else { 0 },
                },
                Some(wgpu::Face::Back),
                fill_params,
                images,
                command.image,
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
                        // Under a clip, only in-clip pixels can carry winding
                        // and 0x80|w > 0x80 exactly when w != 0, so LESS
                        // (reference < stencil) is the whole test; the ZERO
                        // ops are masked to the winding bits below.
                        compare: if command.clip_active {
                            wgpu::CompareFunction::Less
                        } else {
                            wgpu::CompareFunction::NotEqual
                        },
                        fail_op: wgpu::StencilOperation::Zero,
                        depth_fail_op: wgpu::StencilOperation::Zero,
                        pass_op: wgpu::StencilOperation::Zero,
                    },
                    back: wgpu::StencilFaceState {
                        compare: if command.clip_active {
                            wgpu::CompareFunction::Less
                        } else {
                            wgpu::CompareFunction::NotEqual
                        },
                        fail_op: wgpu::StencilOperation::Zero,
                        depth_fail_op: wgpu::StencilOperation::Zero,
                        pass_op: wgpu::StencilOperation::Zero,
                    },
                    read_mask: match (command.clip_active, command.fill_rule) {
                        (false, FillRule::NonZero) => 0xff,
                        (false, FillRule::EvenOdd) => 0x1,
                        (true, FillRule::NonZero) => 0xff,
                        (true, FillRule::EvenOdd) => 0x81,
                    },
                    // Even-odd reads only the parity bit, but the winding pass
                    // wrote the full count (2 in overlaps, 0xff for a wrapped
                    // -1); clear every winding bit so nothing leaks into the
                    // next fill. The clip bit stays write-protected.
                    write_mask: if command.clip_active { 0x7f } else { 0xff },
                },
                stencil_reference: if command.clip_active { 0x80 } else { 0 },
            },
            Some(wgpu::Face::Back),
            fill_params,
            images,
            command.image,
            command.glyph_texture,
        );
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

/// Intersects the persistent stencil clip with the command's path (winding
/// gated on the current clip bit, then two resolve quads), color writes off.
fn clip_fill(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    images: &mut ImageStore<Image>,
) {
    let stencil_params = Params::stencil();

    // Winding, only where the clip bit is currently set; bit 7 protected.
    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        None,
        wgpu::PrimitiveTopology::TriangleList,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::IncrementWrap,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::DecrementWrap,
                },
                read_mask: 0x80,
                write_mask: 0x7f,
            },
            stencil_reference: 0x80,
        },
        None,
        &stencil_params,
        images,
        None,
        GlyphTexture::None,
    );
    for drawable in &command.drawables {
        if let Some((start, count)) = drawable.fill_verts {
            render_pass_builder.draw(start as u32..(start + count) as u32);
        }
    }

    let Some((start, count)) = command.triangles_verts else {
        return;
    };
    let winding_read_mask = match command.fill_rule {
        FillRule::NonZero => 0xff,
        FillRule::EvenOdd => 0x81,
    };

    // Quad 1: clip bit set but no winding -> outside the new clip.
    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        None,
        wgpu::PrimitiveTopology::TriangleStrip,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Zero,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Zero,
                },
                read_mask: winding_read_mask,
                write_mask: 0x80,
            },
            stencil_reference: 0x80,
        },
        None,
        &stencil_params,
        images,
        None,
        GlyphTexture::None,
    );
    render_pass_builder.draw(start as u32..(start + count) as u32);

    // Quad 2: clear the winding bits.
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
                read_mask: 0xff,
                write_mask: 0x7f,
            },
            stencil_reference: 0,
        },
        None,
        &stencil_params,
        images,
        None,
        GlyphTexture::None,
    );
    render_pass_builder.draw(start as u32..(start + count) as u32);
}

/// Rewrites the clip plane with a full-canvas quad: armed (0x80 everywhere)
/// or disarmed back to the zero ambient.
fn clip_reset(
    command: &super::Command,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    images: &mut ImageStore<Image>,
    visible: bool,
) {
    let stencil_params = Params::stencil();
    pipeline_and_bindgroup_mapper.update_renderpass(
        render_pass_builder,
        None,
        wgpu::PrimitiveTopology::TriangleStrip,
        StencilTest::Enabled {
            stencil_state: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Replace,
                    depth_fail_op: wgpu::StencilOperation::Replace,
                    pass_op: wgpu::StencilOperation::Replace,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Replace,
                    depth_fail_op: wgpu::StencilOperation::Replace,
                    pass_op: wgpu::StencilOperation::Replace,
                },
                read_mask: 0xff,
                write_mask: 0xff,
            },
            stencil_reference: if visible { 0x80 } else { 0x00 },
        },
        None,
        &stencil_params,
        images,
        None,
        GlyphTexture::None,
    );
    if let Some((start, count)) = command.triangles_verts {
        render_pass_builder.draw(start as u32..(start + count) as u32);
    }
}

fn convex_fill(
    command: &super::Command,
    fans: u32,
    pipeline_and_bindgroup_mapper: &mut CommandToPipelineAndBindGroupMapper,
    render_pass_builder: &mut RenderPassBuilder<'_>,
    params: &Params,
    images: &mut ImageStore<Image>,
) {
    let blend_state = blend_state(command).into();

    let mut first = fans;
    for drawable in &command.drawables {
        if let Some((_, count)) = drawable.fill_verts {
            pipeline_and_bindgroup_mapper.update_renderpass(
                render_pass_builder,
                blend_state,
                wgpu::PrimitiveTopology::TriangleList,
                clip_guard(command.clip_active),
                Some(wgpu::Face::Back),
                params,
                images,
                command.image,
                command.glyph_texture,
            );
            render_pass_builder.draw_fan(&mut first, count);
        }

        if let Some((start, count)) = drawable.stroke_verts {
            pipeline_and_bindgroup_mapper.update_renderpass(
                render_pass_builder,
                blend_state,
                wgpu::PrimitiveTopology::TriangleStrip,
                clip_guard(command.clip_active),
                Some(wgpu::Face::Back),
                params,
                images,
                command.image,
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
    keep_clip: bool,
    verts: &[super::Vertex],
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
        // A clear of the whole target before anything is drawn into it is the
        // load op of the target's pass: on a tiler each tile then starts out
        // as the color instead of being read from memory and painted over.
        // Keeping a clip plane takes the quad, which zeroes the winding bits
        // only; a stencil load op clears all of them or none.
        if !keep_clip && render_pass_builder.clear_on_load(&verts[start..start + count], params.inner_col) {
            return;
        }
        // Zero the stencil under the cleared rect, as the GL backend's
        // stencil clear does: a winding count a cover pass missed must not
        // leak into the next fills. With a clip armed on the target the clip
        // plane (bit 7) is left alone. A pass with no stencil has none to
        // clear: the next one to attach it clears it as it loads.
        render_pass_builder.ensure_pass();
        let stencil_test = if render_pass_builder.stencil_buffer.is_some() {
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
                    read_mask: 0xff,
                    write_mask: if keep_clip { 0x7f } else { 0xff },
                },
                stencil_reference: 0,
            }
        } else {
            StencilTest::Disabled
        };
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
            stencil_test,
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

/// Stencil test for plain color draws under an armed clip plane: pass only
/// where the clip bit (0x80) is set; never write.
fn clip_guard(active: bool) -> StencilTest {
    if !active {
        return StencilTest::Disabled;
    }
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    StencilTest::Enabled {
        stencil_state: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0x80,
            write_mask: 0,
        },
        stencil_reference: 0x80,
    }
}

// Only what materialize() reads belongs in the key; anything else would split the cache for nothing.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct PipelineState {
    render_to_texture: bool,
    color_target_state: wgpu::ColorTargetState,
    primitive_topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    stencil_state: Option<wgpu::StencilState>,
    /// The draw is under a clip shape, which only its fragment entry point evaluates.
    clip_shape: bool,
    /// The draw is under a coverage mask, which only its fragment entry point reads.
    clip_mask: bool,
}

impl PipelineState {
    fn new(
        color_blend: Option<wgpu::BlendState>,
        stencil_test: StencilTest,
        format: wgpu::TextureFormat,
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
            render_to_texture,
            color_target_state,
            primitive_topology,
            cull_mode,
            stencil_state: has_stencil_buffer.then_some(stencil_state),
            clip_shape: false,
            clip_mask: false,
        }
    }

    #[deny(unused_variables)]
    fn materialize(
        &self,
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        shader_module: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        // Exhaustively bind the key so adding a field or leaving one unused here is a compile error.
        let Self {
            render_to_texture,
            color_target_state,
            primitive_topology,
            cull_mode,
            stencil_state,
            clip_shape,
            clip_mask,
        } = self;
        let vertex_entry_point = if *render_to_texture {
            "vs_main_texture"
        } else {
            "vs_main"
        };
        let fragment_entry_point = match (*clip_shape, *clip_mask) {
            (false, false) => "fs_main",
            (true, false) => "fs_main_clip",
            (false, true) => "fs_main_mask",
            (true, true) => "fs_main_clip_mask",
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
                entry_point: Some(fragment_entry_point),
                compilation_options: Default::default(),
                targets: &[Some(color_target_state.clone())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: *primitive_topology,
                front_face: if *render_to_texture {
                    wgpu::FrontFace::Cw
                } else {
                    wgpu::FrontFace::Ccw
                },
                cull_mode: *cull_mode,
                ..Default::default()
            },
            depth_stencil: stencil_state.as_ref().map(|stencil_state| wgpu::DepthStencilState {
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

/// The layout of the draws' second bind group - their uniforms, image and glyph texture, and under a coverage mask
/// the mask - and the pipeline layout over it and the viewport's. A draw under no mask binds no mask texture: the
/// group is bound again for every draw's uniforms, and a binding more costs each of those binds.
#[derive(Clone, Debug)]
struct DrawLayout {
    bind_group: wgpu::BindGroupLayout,
    pipeline: wgpu::PipelineLayout,
}

impl DrawLayout {
    fn new(device: &wgpu::Device, viewport_bind_group_layout: &wgpu::BindGroupLayout, mask: bool) -> Self {
        let entries = [
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
            // The coverage mask of a clip, read texel by texel.
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ];
        let bind_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries[..if mask { 6 } else { 5 }],
        });
        let pipeline = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(viewport_bind_group_layout), Some(&bind_group)],
            immediate_size: 0,
        });
        Self { bind_group, pipeline }
    }
}

/// The texture a draw samples as its image: one of the canvas's, or a view
/// the renderer holds itself.
#[derive(Clone, PartialEq, Eq, Hash)]
enum MainTexture {
    Image(Option<ImageId>),
    View(wgpu::TextureView),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct BindGroupState {
    image: MainTexture,
    glyph_texture: GlyphTexture,
    clip_mask: Option<ImageId>,
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
        let (main_texture_view, main_sampler) = match &self.image {
            MainTexture::Image(image) => RenderPassBuilder::create_binding_resource_and_sampler(
                device,
                images,
                image.as_ref(),
                empty_texture_view,
                sampler_cache,
            ),
            MainTexture::View(view) => {
                // No image is the default sampler: bilinear, clamped to the edge.
                let (_, sampler) = RenderPassBuilder::create_binding_resource_and_sampler(
                    device,
                    images,
                    None,
                    empty_texture_view,
                    sampler_cache,
                );
                (OwnedBindingResource::TextureView(view.clone()), sampler)
            }
        };
        let (glyph_texture_view, glyph_sampler) = RenderPassBuilder::create_binding_resource_and_sampler(
            device,
            images,
            self.glyph_texture.image_id().as_ref(),
            empty_texture_view,
            sampler_cache,
        );

        // The empty texture stands in for no mask, and the layout without a
        // mask takes no entry for it.
        let (clip_mask_view, _) = RenderPassBuilder::create_binding_resource_and_sampler(
            device,
            images,
            self.clip_mask.as_ref(),
            empty_texture_view,
            sampler_cache,
        );

        if main_texture_view.is_external() || glyph_texture_view.is_external() || clip_mask_view.is_external() {
            unimplemented!("External texture shaders and bind groups are not implemented yet");
        }

        let entries = [
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
            wgpu::BindGroupEntry {
                binding: 5,
                resource: (&clip_mask_view).into(),
            },
        ];
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: bind_group_layout,
            entries: &entries[..if self.clip_mask.is_some() { 6 } else { 5 }],
            label: None,
        })
    }
}

/// Viewport bind groups kept between flushes beyond those the last one bound. One costs about 5 KB to keep and
/// 10 microseconds to make again.
const VIEWPORT_BIND_GROUP_CAPACITY: usize = 128;

/// The viewport uniform of each render target size, bound as group 0 by every
/// pass on a target of that size. A frame's targets repeat their sizes, within
/// the frame and from one frame or flush to the next, so the uniform and bind
/// group made for a size are kept: up to `VIEWPORT_BIND_GROUP_CAPACITY` sizes
/// beyond those the last flush bound, the least recently bound dropped first.
#[derive(Debug)]
struct ViewportBindGroups {
    layout: wgpu::BindGroupLayout,
    /// The bind group of each size and the last flush that bound it.
    groups: HashMap<[u32; 2], (wgpu::BindGroup, u64)>,
}

impl ViewportBindGroups {
    fn new(layout: wgpu::BindGroupLayout) -> Self {
        Self {
            layout,
            groups: HashMap::new(),
        }
    }

    fn get(&mut self, device: &wgpu::Device, viewport: [f32; 2], flush: u64) -> wgpu::BindGroup {
        let (group, last_used) = self
            .groups
            .entry(viewport.map(|extent| extent as u32))
            .or_insert_with(|| (Self::create(device, &self.layout, viewport), flush));
        *last_used = flush;
        group.clone()
    }

    fn evict(&mut self, flush: u64) {
        evict_least_recently_used(
            &mut self.groups,
            flush,
            VIEWPORT_BIND_GROUP_CAPACITY,
            0,
            |(_, last_used)| *last_used,
        );
    }

    fn create(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, viewport: [f32; 2]) -> wgpu::BindGroup {
        // WebGL requires 16 byte alignment for uniforms, so pad accordingly.
        let viewport_padded: [f32; 4] = [viewport[0], viewport[1], 0.0, 0.0];

        let view_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Uniform Buffer for Viewport"),
            contents: bytemuck::cast_slice(&viewport_padded),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buf.as_entire_binding(),
            }],
            label: None,
        })
    }
}

/// A render target's stencil: the texture its passes attach, made by the first
/// pass that needs one, and whether a clip plane armed in it outlives the pass
/// that wrote it. A fill zeroes the winding it counted before the next command
/// draws, so the plane is the only stencil content one pass hands another.
#[derive(Debug, Default)]
struct TargetStencil {
    texture: Option<wgpu::Texture>,
    clip_armed: bool,
}

fn create_stencil(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Stencil buffer"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Stencil8,
        view_formats: &[],
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    })
}

/// Whose stencil the current target's passes attach.
enum StencilTarget {
    Screen,
    Image(wgpu::Texture),
    /// A filter's target: its passes draw no fills and take no clip.
    None,
}

/// What a command does with its target's stencil.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StencilUse {
    /// Ends the pass: a change of target, or a filter or a blur's first
    /// draw, which draw into targets of their own.
    PassEnd,
    /// Draws without it.
    Unused,
    /// Counts winding in it, or tests the clip plane.
    Used,
    /// Arms the clip plane: `ClipReset { visible: true }`.
    Arms,
    /// Disarms the clip plane: `ClipReset { visible: false }`.
    Disarms,
}

impl StencilUse {
    fn of(command: &super::Command) -> Self {
        use super::CommandType;
        match command.cmd_type {
            CommandType::SetRenderTarget(_)
            | CommandType::RenderFilteredImage { .. }
            | CommandType::StartBlur { .. } => Self::PassEnd,
            // A blur's draw back is a fill: concave, or under the clip plane,
            // it takes the stencil as any fill does.
            CommandType::FinishBlur { stencil_params, .. } => {
                if stencil_params.is_some() || command.clip_active {
                    Self::Used
                } else {
                    Self::Unused
                }
            }
            CommandType::ClipReset { visible: true } => Self::Arms,
            CommandType::ClipReset { visible: false } => Self::Disarms,
            CommandType::ClipFill | CommandType::ConcaveFill { .. } | CommandType::StencilStroke { .. } => Self::Used,
            // Without a clip plane the stencil is zero wherever no fill is
            // counting winding, and a pass begins by clearing it: the clear's
            // own stencil clear has nothing left to do.
            CommandType::ClearRect { keep_clip, .. } => {
                if keep_clip {
                    Self::Used
                } else {
                    Self::Unused
                }
            }
            CommandType::ConvexFill { .. } | CommandType::Stroke { .. } | CommandType::Triangles { .. } => {
                if command.clip_active {
                    Self::Used
                } else {
                    Self::Unused
                }
            }
        }
    }

    /// Whether the commands of `uses` up to the end of their pass test or
    /// write the stencil, and whether a clip plane is armed when they are
    /// done, `armed` saying whether one is before them.
    fn ahead(uses: &[Self], mut armed: bool) -> (bool, bool) {
        let mut used = false;
        for &stencil_use in uses {
            match stencil_use {
                Self::PassEnd => break,
                Self::Unused => {}
                Self::Used => used = true,
                Self::Arms => (used, armed) = (true, true),
                Self::Disarms => (used, armed) = (true, false),
            }
        }
        (used, armed)
    }
}

#[cfg(test)]
#[test]
fn a_pass_takes_the_stencil_only_for_commands_that_use_it_and_keeps_only_an_armed_plane() {
    use super::{Command, CommandType};
    use StencilUse::{Arms, Disarms, PassEnd, Unused, Used};

    let of = |cmd_type, clip_active| {
        let mut command = Command::new(cmd_type);
        command.clip_active = clip_active;
        StencilUse::of(&command)
    };
    let params = Params::default();
    assert_eq!(of(CommandType::SetRenderTarget(RenderTarget::Screen), false), PassEnd);
    let finish_blur = |stencil_params| CommandType::FinishBlur {
        stencil_params,
        fill_params: params,
        erase: false,
    };
    assert_eq!(of(finish_blur(None), false), Unused);
    assert_eq!(of(finish_blur(None), true), Used, "tests the clip plane");
    assert_eq!(of(finish_blur(Some(params)), false), Used, "counts winding");
    assert_eq!(of(CommandType::ConvexFill { params }, false), Unused);
    assert_eq!(
        of(CommandType::ConvexFill { params }, true),
        Used,
        "tests the clip plane"
    );
    assert_eq!(
        of(
            CommandType::ConcaveFill {
                stencil_params: params,
                fill_params: params
            },
            false
        ),
        Used
    );
    assert_eq!(of(CommandType::ClipReset { visible: true }, false), Arms);
    assert_eq!(of(CommandType::ClipReset { visible: false }, true), Disarms);
    let clear = |keep_clip| CommandType::ClearRect {
        color: crate::Color::white(),
        keep_clip,
    };
    assert_eq!(
        of(clear(false), false),
        Unused,
        "the next pass clears the stencil anyway"
    );
    assert_eq!(of(clear(true), true), Used, "keeps the plane, zeroes the winding bits");

    // Whether the pass uses the stencil, and whether a plane is armed as it ends.
    assert_eq!(
        StencilUse::ahead(&[Unused, Unused, PassEnd, Used], false),
        (false, false)
    );
    assert_eq!(StencilUse::ahead(&[Unused, Used, PassEnd, Arms], false), (true, false));
    assert_eq!(StencilUse::ahead(&[Arms, Used], false), (true, true));
    assert_eq!(StencilUse::ahead(&[Used, Disarms, Used], true), (true, false));
    assert_eq!(
        StencilUse::ahead(&[Unused], true),
        (false, true),
        "an armed plane outlives a pass that does not attach it"
    );
    assert_eq!(
        StencilUse::ahead(&[], false),
        (false, false),
        "a clear begun at the end of the commands"
    );
}

struct RenderPassBuilder<'a> {
    device: wgpu::Device,
    frame: FrameEncoder<'a>,
    surface_view: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,
    texture_view: wgpu::TextureView,
    /// The stencil the open pass attaches, if any.
    stencil_buffer: Option<wgpu::Texture>,
    /// Whose stencil the current target's next pass attaches if it needs one.
    stencil_target: StencilTarget,
    viewport: [f32; 2],
    vertex_buffer: wgpu::Buffer,
    fan_indices: wgpu::Buffer,
    rendering_to_texture: bool,
    viewport_bind_groups: &'a mut ViewportBindGroups,
    flush_count: u64,
    rpass: Option<wgpu::RenderPass<'a>>,
    /// The load op the current target's pass begins with, while no draw has
    /// begun it: a clear of the whole target before any draw is one, see
    /// `clear_on_load`. A pass that loads and stores its target without drawing
    /// changes nothing and is not free - on Metal it is a command buffer of
    /// its own, see `SubmissionSlicing` - so a target that is left again
    /// before anything is drawn into it gets no pass.
    pending_load: Option<wgpu::LoadOp<wgpu::Color>>,
    screen_stencil: &'a mut TargetStencil,
    screen_view: [f32; 2],
    screen_surface_format: wgpu::TextureFormat,
    stencil_buffer_for_textures: &'a mut HashMap<wgpu::Texture, TargetStencil>,
    /// Each command's use of the stencil, and the one being drawn: a pass
    /// that begins looks ahead to its end.
    stencil_uses: Vec<StencilUse>,
    command_index: usize,
    viewport_bind_group: wgpu::BindGroup,
    current_pipeline_state: Option<PipelineState>,
    current_stencil_reference: Option<u32>,
    current_bound_offset: Option<u32>,
    // The scissor rect set on the open pass; none is the whole target.
    current_clip_bounds: Option<[u32; 4]>,
    /// What the draws that follow are scissored to: what the current
    /// command's clip leaves of the target.
    clip_bounds: Option<[u32; 4]>,
}

impl<'a> RenderPassBuilder<'a> {
    fn new(
        device: wgpu::Device,
        frame: FrameEncoder<'a>,
        screen_surface_format: wgpu::TextureFormat,
        screen_view: [f32; 2],
        viewport_bind_groups: &'a mut ViewportBindGroups,
        flush_count: u64,
        screen_stencil: &'a mut TargetStencil,
        stencil_buffer_for_textures: &'a mut HashMap<wgpu::Texture, TargetStencil>,
        stencil_uses: Vec<StencilUse>,
        texture_view: wgpu::TextureView,
        vertex_buffer: wgpu::Buffer,
        fan_indices: wgpu::Buffer,
    ) -> Self {
        let viewport_bind_group = viewport_bind_groups.get(&device, screen_view, flush_count);
        Self {
            device: device.clone(),
            frame,
            surface_view: texture_view.clone(),
            surface_format: screen_surface_format,
            texture_view,
            stencil_buffer: None,
            stencil_target: StencilTarget::Screen,
            viewport: screen_view,
            vertex_buffer,
            fan_indices,
            rendering_to_texture: false,
            viewport_bind_groups,
            flush_count,
            rpass: None,
            pending_load: None,
            screen_stencil,
            screen_view,
            screen_surface_format,
            stencil_buffer_for_textures,
            stencil_uses,
            command_index: 0,
            viewport_bind_group,
            current_pipeline_state: None,
            current_stencil_reference: None,
            current_bound_offset: None,
            current_clip_bounds: None,
            clip_bounds: None,
        }
    }

    /// Finishes the frame's last command buffer; see `FrameEncoder::finish`.
    fn finish(mut self) -> (wgpu::CommandBuffer, Vec<u32>) {
        self.end_pass();
        self.frame.finish()
    }

    /// Ends the current target's pass. One that no draw began is begun here
    /// only if it clears, which has to happen without a draw too.
    fn end_pass(&mut self) {
        if matches!(self.pending_load, Some(wgpu::LoadOp::Clear(_))) {
            self.ensure_pass();
        }
        self.pending_load = None;
        drop(self.rpass.take());
    }

    /// Scissors the target's draws to the pixels `command`'s clip leaves
    /// them, or to the whole target again.
    fn set_clip_bounds(&mut self, command: &super::Command) {
        self.clip_bounds = command.clip_bounds([self.viewport[0] as u32, self.viewport[1] as u32]);
        if self.rpass.is_some() {
            self.apply_clip_bounds();
        }
    }

    fn apply_clip_bounds(&mut self) {
        if self.current_clip_bounds == self.clip_bounds {
            return;
        }
        self.current_clip_bounds = self.clip_bounds;
        let (width, height) = (self.viewport[0] as u32, self.viewport[1] as u32);
        let [x, y, w, h] = self.clip_bounds.unwrap_or([0, 0, width, height]);
        // An image target stores its rows bottom up.
        let y = if self.rendering_to_texture { height - (y + h) } else { y };
        self.rpass.as_mut().unwrap().set_scissor_rect(x, y, w, h);
    }

    fn set_viewport(&mut self, viewport: [f32; 2]) {
        if self.viewport == viewport {
            return;
        }
        self.viewport = viewport;
        self.viewport_bind_group = self.viewport_bind_groups.get(&self.device, viewport, self.flush_count);
    }

    fn create_binding_resource_and_sampler(
        device: &wgpu::Device,
        images: &ImageStore<Image>,
        image: Option<&ImageId>,
        empty_texture_view: &wgpu::TextureView,
        sampler_cache: &RefCell<HashMap<crate::ImageFlags, wgpu::Sampler>>,
    ) -> (OwnedBindingResource, wgpu::Sampler) {
        let flags = image
            .and_then(|image_id| images.get(*image_id).map(|img| img.info.flags()))
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
                    // Across the levels the way OpenGL's LINEAR_MIPMAP_LINEAR
                    // and NEAREST_MIPMAP_NEAREST go; a single-level image
                    // has nothing to filter between.
                    mipmap_filter: match (flags.contains(crate::ImageFlags::GENERATE_MIPMAPS), filter_mode) {
                        (true, wgpu::FilterMode::Linear) => wgpu::MipmapFilterMode::Linear,
                        _ => wgpu::MipmapFilterMode::Nearest,
                    },
                    ..Default::default()
                })
            })
            .clone();

        let binding_resource = image
            .and_then(|image_id| {
                images.get(*image_id).map(|img| match &img.texture {
                    Texture::Internal(texture) => {
                        OwnedBindingResource::TextureView(texture.create_view(&Default::default()))
                    }
                    Texture::External(texture) => OwnedBindingResource::ExternalTexture(texture.clone()),
                })
            })
            .unwrap_or_else(|| OwnedBindingResource::TextureView(empty_texture_view.clone()));

        (binding_resource, sampler)
    }

    fn set_render_target_texture(
        &mut self,
        texture: &wgpu::Texture,
        stencil_target: StencilTarget,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        self.end_pass();
        self.texture_view = texture.create_view(&base_level());
        self.set_viewport([texture.width() as f32, texture.height() as f32]);
        self.stencil_target = stencil_target;
        self.surface_format = texture.format();
        self.rendering_to_texture = true;

        self.defer_pass(load);
    }

    fn set_render_target_image(
        &mut self,
        images: &mut ImageStore<Image>,
        image_id: ImageId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let image = images.get(image_id).unwrap();

        if let Texture::Internal(texture) = &image.texture {
            let texture = texture.clone();
            self.set_render_target_texture(&texture, StencilTarget::Image(texture.clone()), load);
        }
    }

    fn set_filter_target_image(
        &mut self,
        images: &ImageStore<Image>,
        image_id: ImageId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let image = images.get(image_id).unwrap();
        if let Texture::Internal(texture) = &image.texture {
            self.set_render_target_texture(texture, StencilTarget::None, load);
        }
    }

    fn set_render_target_screen(&mut self) {
        self.end_pass();
        self.texture_view = self.surface_view.clone();
        self.stencil_target = StencilTarget::Screen;
        self.set_viewport(self.screen_view);
        self.surface_format = self.screen_surface_format;
        self.rendering_to_texture = false;

        self.defer_pass(wgpu::LoadOp::Load);
    }

    /// Ends the current target's pass - beginning one that only clears, so
    /// that the clear lands first - for work the encoder records outside a
    /// pass; the next draw on the target begins another.
    fn end_render_pass(&mut self) -> &mut wgpu::CommandEncoder {
        self.end_pass();
        self.defer_pass(wgpu::LoadOp::Load);
        &mut self.frame.encoder
    }

    /// Leaves the pass on the target just set to the first draw into it.
    fn defer_pass(&mut self, load: wgpu::LoadOp<wgpu::Color>) {
        // A new render pass resets state, so nothing set on the previous one still counts.
        self.current_pipeline_state = None;
        self.current_stencil_reference = None;
        self.current_bound_offset = None;
        self.current_clip_bounds = None;
        self.clip_bounds = None;
        self.pending_load = Some(load);
    }

    /// Makes a clear of the rect `verts` outline to `color` (what its quad
    /// writes) the load op of the current target's pass, if no draw has
    /// begun the pass and the rect covers the target; returns whether it did.
    fn clear_on_load(&mut self, verts: &[super::Vertex], [r, g, b, a]: [f32; 4]) -> bool {
        if self.rpass.is_some() {
            return false;
        }
        let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for vertex in verts {
            min = [min[0].min(vertex.x), min[1].min(vertex.y)];
            max = [max[0].max(vertex.x), max[1].max(vertex.y)];
        }
        if min[0] > 0.0 || min[1] > 0.0 || max[0] < self.viewport[0] || max[1] < self.viewport[1] {
            return false;
        }
        self.pending_load = Some(wgpu::LoadOp::Clear(wgpu::Color {
            r: r.into(),
            g: g.into(),
            b: b.into(),
            a: a.into(),
        }));
        true
    }

    /// Begins the current target's pass unless a draw already has.
    fn ensure_pass(&mut self) {
        if self.rpass.is_some() {
            return;
        }
        let load = self.pending_load.take().unwrap_or(wgpu::LoadOp::Load);
        self.frame.begin_pass();
        let stencil = self.stencil_for_pass();
        self.stencil_buffer = stencil.as_ref().map(|(texture, _)| texture.clone());
        let stencil_view = stencil
            .as_ref()
            .map(|(texture, ops)| (texture.create_view(&Default::default()), *ops));

        let mut rpass = self.frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                .map(|(view, ops)| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: None,
                    stencil_ops: Some(*ops),
                }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.fan_indices.slice(..), wgpu::IndexFormat::Uint32);
        rpass.set_viewport(0., 0., self.viewport[0], self.viewport[1], 0., 0.);
        rpass.set_bind_group(0, &self.viewport_bind_group, &[]);
        self.rpass = Some(rpass.forget_lifetime());
        self.apply_clip_bounds();
    }

    /// The stencil the pass about to begin attaches, and how. A pass that
    /// draws nothing testing or writing it attaches none: on a tiler a
    /// stencil attachment costs tile memory, and the target keeps any clip
    /// plane in its texture for a later pass. A pass that does attach one
    /// clears it on load unless a clip plane is armed when it begins, and
    /// discards it on store unless one is armed when it ends, so that on a
    /// tiler the stencil is neither read from memory nor written back.
    fn stencil_for_pass(&mut self) -> Option<(wgpu::Texture, wgpu::Operations<u32>)> {
        let (state, width, height) = match &self.stencil_target {
            StencilTarget::None => return None,
            StencilTarget::Screen => (
                &mut *self.screen_stencil,
                self.screen_view[0] as u32,
                self.screen_view[1] as u32,
            ),
            StencilTarget::Image(texture) => (
                self.stencil_buffer_for_textures.entry(texture.clone()).or_default(),
                texture.width(),
                texture.height(),
            ),
        };
        let armed = state.clip_armed;
        let ahead = self.stencil_uses.get(self.command_index..).unwrap_or_default();
        let (used, armed_at_end) = StencilUse::ahead(ahead, armed);
        if !used {
            return None;
        }
        state.clip_armed = armed_at_end;
        let texture = state
            .texture
            .get_or_insert_with(|| create_stencil(&self.device, width, height))
            .clone();
        let ops = wgpu::Operations {
            load: if armed {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(0)
            },
            store: if armed_at_end {
                wgpu::StoreOp::Store
            } else {
                wgpu::StoreOp::Discard
            },
        };
        Some((texture, ops))
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>) {
        self.ensure_pass();
        self.rpass.as_mut().unwrap().draw(vertices, 0..1);
    }

    /// Draws the fan of `count` vertices whose triangles start at `first` in
    /// the frame's fan indices, and moves `first` past them.
    fn draw_fan(&mut self, first: &mut u32, count: usize) {
        let end = *first + FanIndices::count(count);
        self.rpass.as_mut().unwrap().draw_indexed(*first..end, 0, 0..1);
        *first = end;
    }

    /// Limits the open pass to `crop` - x, y, width and height in the
    /// target's texel rows - and says whether that leaves anything to draw.
    fn crop(&mut self, crop: Option<[u32; 4]>) -> bool {
        let Some([x, y, width, height]) = crop else {
            return true;
        };
        if width == 0 || height == 0 {
            return false;
        }
        self.ensure_pass();
        self.rpass.as_mut().unwrap().set_scissor_rect(x, y, width, height);
        true
    }
}

struct CommandToPipelineAndBindGroupMapper {
    device: wgpu::Device,
    empty_texture_view: wgpu::TextureView,
    sampler_cache: SamplerCache,
    uniform_buffer: wgpu::Buffer,
    current_uniforms: Option<UniformArray>,
    shader_module: Rc<wgpu::ShaderModule>,

    current_bind_group_state: Option<BindGroupState>,
    current_bind_group: Option<wgpu::BindGroup>,
    /// The flush's bind groups by what they bind. Draws alternate between a
    /// few states - a concave fill's winding pass carries no soft clip mask
    /// and its cover does, a glyph quad binds its atlas between fills - so
    /// each is made once a flush, not at every change. The images and the
    /// uniform buffer they hold do not change within a flush.
    bind_groups: FnvHashMap<BindGroupState, wgpu::BindGroup>,
    /// Without and with a coverage mask.
    draw_layouts: [DrawLayout; 2],
    pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
    flush_count: u64,
}

impl CommandToPipelineAndBindGroupMapper {
    fn new(
        device: wgpu::Device,
        empty_texture_view: wgpu::TextureView,
        sampler_cache: SamplerCache,
        uniform_buffer: wgpu::Buffer,
        shader_module: Rc<wgpu::ShaderModule>,
        draw_layouts: [DrawLayout; 2],
        pipeline_cache: Rc<RefCell<HashMap<PipelineState, CachedPipeline>>>,
        flush_count: u64,
    ) -> Self {
        Self {
            device: device.clone(),
            empty_texture_view,
            sampler_cache,
            uniform_buffer,
            current_uniforms: None,
            shader_module,
            current_bind_group_state: None,
            current_bind_group: None,
            bind_groups: FnvHashMap::default(),
            draw_layouts,
            pipeline_cache,
            flush_count,
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
        image: Option<ImageId>,
        glyph_texture: GlyphTexture,
    ) {
        self.update_renderpass_sampling(
            render_pass_builder,
            color_blend,
            primitive_topology,
            stencil_test,
            cull_mode,
            params,
            images,
            MainTexture::Image(image),
            glyph_texture,
        );
    }

    /// [`update_renderpass`](Self::update_renderpass) for a draw that may
    /// sample a view the renderer holds rather than an image.
    fn update_renderpass_sampling<'a>(
        &mut self,
        render_pass_builder: &'a mut RenderPassBuilder<'_>,
        color_blend: Option<wgpu::BlendState>,
        primitive_topology: wgpu::PrimitiveTopology,
        stencil_test: StencilTest,
        cull_mode: Option<wgpu::Face>,
        params: &Params,
        images: &'a ImageStore<Image>,
        image: MainTexture,
        glyph_texture: GlyphTexture,
    ) {
        render_pass_builder.ensure_pass();
        debug_assert!(
            render_pass_builder.stencil_buffer.is_some() || stencil_test == StencilTest::Disabled,
            "a draw tests or writes the stencil in a pass that attaches none: StencilUse::of misses its command"
        );
        let render_pass = render_pass_builder.rpass.as_mut().unwrap();

        let stencil_reference = match &stencil_test {
            StencilTest::Enabled { stencil_reference, .. } => *stencil_reference,
            _ => 0,
        };
        if render_pass_builder.current_stencil_reference != Some(stencil_reference) {
            render_pass.set_stencil_reference(stencil_reference);
            render_pass_builder.current_stencil_reference = Some(stencil_reference);
        }

        let bind_group_state = BindGroupState {
            image,
            glyph_texture,
            clip_mask: params.clip_mask.map(|mask| mask.image),
        };

        let bind_group_changed = self.current_bind_group_state.as_ref() != Some(&bind_group_state);
        if bind_group_changed {
            let bind_group = self.bind_groups.entry(bind_group_state.clone()).or_insert_with(|| {
                bind_group_state.materialize(
                    &self.device,
                    images,
                    &self.draw_layouts[usize::from(bind_group_state.clip_mask.is_some())].bind_group,
                    &self.empty_texture_view,
                    &self.sampler_cache,
                    &self.uniform_buffer,
                )
            });
            self.current_bind_group = Some(bind_group.clone());
            self.current_bind_group_state = Some(bind_group_state);
        }

        let uniforms = UniformArray::from(params);
        if self.current_uniforms.as_ref() != Some(&uniforms) {
            render_pass_builder
                .frame
                .stage_uniforms(bytemuck::cast_slice(uniforms.as_slice()));
            self.current_uniforms = Some(uniforms);
        }
        // The current command's slot is the last one staged.
        let offset = render_pass_builder.frame.current_uniform_offset();
        if bind_group_changed || render_pass_builder.current_bound_offset != Some(offset) {
            render_pass.set_bind_group(1, self.current_bind_group.as_ref().unwrap(), &[offset]);
            render_pass_builder.current_bound_offset = Some(offset);
        }

        let mut pipeline_state = PipelineState::new(
            color_blend,
            stencil_test,
            render_pass_builder.surface_format,
            render_pass_builder.rendering_to_texture,
            primitive_topology,
            cull_mode,
            render_pass_builder.stencil_buffer.is_some(),
        );
        // Set in place: a copy of the state with the flags changed was slower
        // to hash, by 1.3 % of the time 600 unclipped fills take to encode.
        pipeline_state.clip_shape = params.clip.is_some();
        pipeline_state.clip_mask = params.clip_mask.is_some();

        // An unchanged pipeline was looked up, and stamped with this flush, when it was bound.
        if render_pass_builder.current_pipeline_state.as_ref() != Some(&pipeline_state) {
            let mut pipeline_cache = self.pipeline_cache.borrow_mut();
            let render_pipeline = pipeline_cache.entry(pipeline_state.clone()).or_insert_with(|| {
                let layout = &self.draw_layouts[usize::from(pipeline_state.clip_mask)].pipeline;
                let pipeline = pipeline_state.materialize(&self.device, layout, &self.shader_module);
                CachedPipeline {
                    pipeline,
                    last_used: self.flush_count,
                }
            });
            render_pipeline.last_used = self.flush_count;
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
