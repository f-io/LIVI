//! Module containing renderer implementations.

use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    geometry::Position, paint::GlyphTexture, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageId,
    ImageInfo, ImageSource, ImageStore,
};

mod opengl;
pub use opengl::OpenGl;

#[cfg(feature = "wgpu")]
mod wgpu;
#[cfg(feature = "wgpu")]
pub use wgpu::{SubmissionSlicing, WGPURenderOutput, WGPURenderer};

mod void;
pub use void::Void;

mod params;
pub(crate) use params::Params;

/// Represents a drawable object.
#[derive(Copy, Clone, Default, Debug)]
pub struct Drawable {
    /// A fill's fan, or in a `ClipFill` the triangles of all its contours' fans.
    pub(crate) fill_verts: Option<(usize, usize)>,
    pub(crate) stroke_verts: Option<(usize, usize)>,
}

/// A frame's fill fans as one triangle list over its vertices, for a
/// renderer that draws them indexed: no fan topology, base vertex or rebind
/// is needed, and a layered driver has no fan to emulate per draw.
#[derive(Debug, Default)]
pub(crate) struct FanIndices {
    pub(crate) triangles: Vec<[u32; 3]>,
    // Where each command's fans start, as an index, by the command's place.
    pub(crate) firsts: Vec<u32>,
}

impl FanIndices {
    pub(crate) fn build(&mut self, commands: &[Command]) {
        self.triangles.clear();
        self.firsts.clear();
        for command in commands {
            self.firsts.push(3 * self.triangles.len() as u32);
            if matches!(
                command.cmd_type,
                CommandType::ConvexFill { .. } | CommandType::ConcaveFill { .. } | CommandType::FinishBlur { .. }
            ) {
                for (start, count) in command.drawables.iter().filter_map(|drawable| drawable.fill_verts) {
                    let (hub, end) = (start as u32, (start + count) as u32);
                    self.triangles
                        .extend((hub + 1..end.saturating_sub(1)).map(|rim| [hub, rim, rim + 1]));
                }
            }
        }
    }

    pub(crate) fn indices(&self) -> &[u32] {
        self.triangles.as_flattened()
    }

    /// How many indices the fan of `count` vertices takes.
    pub(crate) fn count(count: usize) -> u32 {
        (count.saturating_sub(2) * 3) as u32
    }
}

/// Defines different types of commands that can be executed by the renderer.
#[derive(Debug)]
pub enum CommandType {
    /// Intersects the persistent stencil clip with a path: the drawables carry
    /// the winding fan triangles and `triangles_verts` the resolve quad over
    /// the previously visible clip bounds. `fill_rule` is the clip-rule.
    ClipFill,
    /// Rewrites the persistent stencil clip with a full-canvas quad
    /// (`triangles_verts`): `visible: true` arms the clip plane (everything
    /// visible, ambient value 0x80), `false` disarms it back to the zero
    /// ambient so clip-free rendering pays nothing.
    ClipReset {
        /// Whether the plane resets to "everything visible" (armed) or to the
        /// disarmed zero state.
        visible: bool,
    },
    /// Set the render target (screen or image).
    SetRenderTarget(RenderTarget),
    /// Clear a rectangle with the specified color.
    ClearRect {
        /// Color to fill the rectangle with.
        color: Color,
        /// A clip is armed on the target: clear only the stencil's winding
        /// bits so the clip plane (bit 7) survives. Otherwise the whole
        /// stencil is cleared, the tile clear a tiler does for free.
        keep_clip: bool,
    },
    /// Fill a convex shape.
    ConvexFill {
        /// Rendering parameters for the fill operation.
        params: Params,
    },
    /// Fill a concave shape.
    ConcaveFill {
        /// Rendering parameters for the stencil operation.
        stencil_params: Params,
        /// Rendering parameters for the fill operation.
        fill_params: Params,
    },
    /// Stroke a shape.
    Stroke {
        /// Rendering parameters for the stroke operation.
        params: Params,
    },
    /// Stroke a shape using stencil.
    StencilStroke {
        /// Rendering parameters for the first stroke operation.
        params1: Params,
        /// Rendering parameters for the second stroke operation.
        params2: Params,
    },
    /// Render triangles.
    Triangles {
        /// Rendering parameters for the triangle operation.
        params: Params,
    },
    /// Render a filtered image.
    RenderFilteredImage {
        /// ID of the target image.
        target_image: ImageId,
        /// Image filter to apply.
        filter: ImageFilter,
    },
    /// First draw of a blur: the render target blurred along x by `sigma`
    /// (zero resamples) into all of `target_image`, with
    /// [`ShaderType::FillBlur`]. The quad is `triangles_verts`.
    StartBlur {
        /// The image drawn into.
        target_image: ImageId,
        /// The pixels the draw reads, `[x, y, width, height]` in device
        /// pixels, within the target: what a renderer that can't sample the
        /// target copies first.
        src: [u32; 4],
        /// The standard deviation along x.
        sigma: f32,
        /// The map from the pixels of `target_image` to the render target
        /// pixels they sample, counted from the top: a renderer storing the
        /// target's rows bottom-up mirrors them. Taps past the target's edges
        /// clamp to the edge pixels.
        sample_transform: crate::Transform2D,
    },
    /// Last draws of a blur, each filled through the drawables as
    /// `ConvexFill` (no `stencil_params`) or `ConcaveFill` would: with
    /// `erase`, first `fill_params` as black at its alpha under
    /// [`CompositeOperation::DestinationOut`], then `image`, blurred along
    /// y and sampled through `fill_params`, under
    /// [`CompositeOperation::Lighter`] after an erase and
    /// [`CompositeOperation::Copy`] without one. The command's own composite
    /// operation doesn't apply. A renderer whose last `StartBlur` didn't draw
    /// skips both: `image` then holds whatever was there before.
    FinishBlur {
        /// Rendering parameters for the stencil operation of a concave path.
        stencil_params: Option<Params>,
        /// Rendering parameters for the fill operation.
        fill_params: Params,
        /// Whether the fill's coverage can be partial: mixing the target
        /// toward the blur by it takes an erase first, as one blend can't do
        /// it for alpha.
        erase: bool,
    },
}

/// The colour matrices a two-draw filter pass carries in its draws: an
/// alpha-only matrix before it, so every tap of the first draw reads
/// (0, 0, 0, a), and any matrix after it, applied by the second draw to its
/// result before storing it - each one draw fewer than its own pass.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fused {
    pub(crate) source_alpha: bool,
    pub(crate) post_matrix: Option<[f32; 20]>,
}

/// A blend pass's inputs beyond its mode: whether the backdrop (the glyph
/// texture) is stored the other way up from the image, the alpha the image
/// is scaled by first, and whether to write the image's contribution over
/// the backdrop - what source-over onto it adds - instead of the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlendPass {
    pub(crate) backdrop_flipped: bool,
    pub(crate) source_alpha: f32,
    pub(crate) contribution: bool,
}

impl Default for BlendPass {
    fn default() -> Self {
        Self {
            backdrop_flipped: false,
            source_alpha: 1.0,
            contribution: false,
        }
    }
}

/// Represents a command that can be executed by the renderer.
#[derive(Debug)]
pub struct Command {
    pub(crate) cmd_type: CommandType,
    // Whether the persistent stencil clip (Canvas::clip_path) applies to this
    // command's fragments. Set centrally when the command is appended.
    pub(crate) clip_active: bool,
    pub(crate) drawables: Vec<Drawable>,
    pub(crate) triangles_verts: Option<(usize, usize)>,
    pub(crate) image: Option<ImageId>,
    pub(crate) filter_scratch: Option<ImageId>,
    pub(crate) glyph_texture: GlyphTexture,
    // A blend pass's inputs beyond its mode; the backdrop is the glyph texture.
    pub(crate) blend_pass: BlendPass,
    // The matrices a two-draw filter pass folds into its draws.
    pub(crate) fused: Fused,
    // The rect a filter pass's result is drawn inside - x, y, width and
    // height in the texel rows of its target - when it is cropped.
    pub(crate) crop: Option<[u32; 4]>,
    pub(crate) fill_rule: FillRule,
    pub(crate) composite_operation: CompositeOperationState,
}

impl Command {
    /// Creates a new command with the specified command type.
    pub fn new(flavor: CommandType) -> Self {
        Self {
            cmd_type: flavor,
            clip_active: false,
            drawables: Vec::new(),
            triangles_verts: None,
            image: None,
            filter_scratch: None,
            glyph_texture: GlyphTexture::default(),
            blend_pass: BlendPass::default(),
            fused: Fused::default(),
            crop: None,
            fill_rule: FillRule::default(),
            composite_operation: CompositeOperationState::default(),
        }
    }

    /// The fills a `FinishBlur` draws, in order, each under its composite
    /// operation: see `CommandType::FinishBlur`.
    pub(crate) fn finish_blur_draws(&self) -> impl Iterator<Item = (CompositeOperationState, Params)> {
        let (fill_params, erase) = match self.cmd_type {
            CommandType::FinishBlur { fill_params, erase, .. } => (fill_params, erase),
            _ => unreachable!("only a `FinishBlur` draws its blur"),
        };
        // After the erase, the target is scaled by one minus the coverage, and
        // the blur adds to it.
        let (erase, blur_operation) = if erase {
            let erase = Params {
                inner_col: [0.0, 0.0, 0.0, fill_params.inner_col[3]],
                shader_type: ShaderType::FillColor,
                ..fill_params
            };
            (
                Some((crate::CompositeOperation::DestinationOut, erase)),
                crate::CompositeOperation::Lighter,
            )
        } else {
            (None, crate::CompositeOperation::Copy)
        };
        erase
            .into_iter()
            .chain([(blur_operation, fill_params)])
            .map(|(operation, params)| (CompositeOperationState::new(operation), params))
    }

    /// The pixels of a target of `size` that the clip leaves the command's
    /// draws - x, y, width and height from the top left - when they carry a
    /// shape that ends somewhere, a mask, or both. Outside them the clip's
    /// coverage is zero, so the renderer scissors every draw of the command
    /// to them, the ones that only write the stencil too: no fragment is
    /// shaded out there and a tiler leaves its tiles alone, as under a clip
    /// on the stencil. Never less than a pixel: Metal takes no empty
    /// scissor.
    pub(crate) fn clip_bounds(&self, [width, height]: [u32; 2]) -> Option<[u32; 4]> {
        let params = match &self.cmd_type {
            CommandType::ConvexFill { params }
            | CommandType::Stroke { params }
            | CommandType::Triangles { params }
            | CommandType::ConcaveFill {
                fill_params: params, ..
            }
            | CommandType::FinishBlur {
                fill_params: params, ..
            }
            | CommandType::StencilStroke { params1: params, .. } => params,
            _ => return None,
        };
        let shape = params.clip.and_then(|shape| shape.reach());
        let mask = params.clip_mask.map(|mask| mask.reach());
        let reach = match (shape, mask) {
            (Some(shape), Some(mask)) => shape.intersect(mask),
            (shape, mask) => shape.or(mask)?,
        };
        if width == 0 || height == 0 {
            return None;
        }
        // A float past either end saturates, and one that is no number is zero.
        let left = (reach.minx.floor() as u32).min(width - 1);
        let top = (reach.miny.floor() as u32).min(height - 1);
        let right = (reach.maxx.ceil() as u32).clamp(left + 1, width);
        let bottom = (reach.maxy.ceil() as u32).clamp(top + 1, height);
        Some([left, top, right - left, bottom - top])
    }
}

/// Represents different render targets (screen or image).
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub enum RenderTarget {
    /// Render to the screen.
    Screen,
    /// Render to a specific image.
    Image(ImageId),
}

/// The main renderer trait that the [Canvas](../struct.Canvas.html) draws to.
pub trait Renderer {
    /// Associated image type.
    type Image;

    /// Associated native texture type.
    type NativeTexture;

    /// Associated external texture type.
    type ExternalTexture;

    /// Associated render output type.
    type RenderOutput;

    /// Associated type to hold commands created via `flush_to_output`.
    type CommandBuffer;

    /// Set the size of the renderer.
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    /// Render the specified commands.
    fn render(
        &mut self,
        output: impl Into<Self::RenderOutput>,
        images: &mut ImageStore<Self::Image>,
        verts: &[Vertex],
        commands: Vec<Command>,
    ) -> Self::CommandBuffer;

    /// Allocate a new image with the specified image info.
    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind>;

    /// Create a new image from a native texture.
    fn create_image_from_native_texture(
        &mut self,
        native_texture: Self::NativeTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind>;

    /// Create a new image from a external texture.
    fn create_image_from_external_texture(
        &mut self,
        native_texture: Self::ExternalTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind>;

    /// Update an image with new data.
    ///
    /// Implementations should start by calling [`ImageSource::check_update`], so
    /// that a copy reaching past the image, or carrying a pixel format the image
    /// was not created with, is reported as an error instead of being handed to
    /// the graphics API, where it fails validation and usually aborts.
    fn update_image(&mut self, image: &mut Self::Image, data: ImageSource, x: usize, y: usize)
        -> Result<(), ErrorKind>;

    /// Get the native texture associated with an image (default implementation returns an error).
    #[allow(unused_variables)]
    fn get_native_texture(&self, image: &Self::Image) -> Result<Self::NativeTexture, ErrorKind> {
        Err(ErrorKind::UnsupportedImageFormat)
    }

    /// Delete an image.
    fn delete_image(&mut self, image: Self::Image, image_id: ImageId);

    /// Take a screenshot of the current render target.
    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind>;

    /// The largest width or height this backend can allocate for an image, in
    /// pixels. Layers and shadows whose stores would exceed it degrade rather
    /// than fail. The default matches current desktop GPUs; a VideoCore IV
    /// (Raspberry Pi Zero through 3) reports 2048.
    fn max_texture_size(&self) -> usize {
        8192
    }

    /// Backend allocation charged when the transient pool creates `info`.
    /// Renderers override this for attachments or scratch reserved alongside
    /// pooled images.
    fn transient_image_cost(&self, info: ImageInfo) -> usize {
        let bytes_per_pixel = match info.format() {
            crate::PixelFormat::Gray8 => 1,
            crate::PixelFormat::Rgb8 => 3,
            crate::PixelFormat::Rgba8 => 4,
        };
        info.width()
            .saturating_mul(info.height())
            .saturating_mul(bytes_per_pixel)
    }
}

/// Marker trait for renderers that don't have a surface.
pub trait SurfacelessRenderer: Renderer {
    /// Render the specified commands.
    fn render_surfaceless(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>);
}

use bytemuck::{Pod, Zeroable};

/// Vertex struct for specifying triangle geometry.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Default, Pod, Zeroable)]
#[repr(C)]
pub struct Vertex {
    /// X-coordinate of the vertex.
    pub x: f32,
    /// Y-coordinate of the vertex.
    pub y: f32,
    /// U-coordinate of the vertex (for texture mapping).
    pub u: f32,
    /// V-coordinate of the vertex (for texture mapping).
    pub v: f32,
}

impl Vertex {
    pub(crate) fn pos(position: Position, u: f32, v: f32) -> Self {
        let Position { x, y } = position;
        Self { x, y, u, v }
    }

    /// Create a new vertex with the specified coordinates.
    pub fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
        Self { x, y, u, v }
    }

    /// Set the coordinates of the vertex.
    pub fn set(&mut self, x: f32, y: f32, u: f32, v: f32) {
        *self = Self { x, y, u, v };
    }
}

/// Represents different types of shaders used by the renderer.
///
/// The default value is `FillGradient`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum ShaderType {
    /// Fill gradient shader.
    #[default]
    FillGradient,
    /// Fill image shader.
    FillImage,
    /// Stencil shader.
    Stencil,
    /// Fill image gradient shader.
    FillImageGradient,
    /// Filter image shader.
    FilterImage,
    /// Fill color shader.
    FillColor,
    /// Texture copy unclipped shader.
    TextureCopyUnclipped,
    /// Fill color shader without clipping, used for `clear_rect()`
    FillColorUnclipped,
    /// Fill conic gradient shader.
    FillGradientConic,
    /// Fill image conic gradient shader.
    FillImageGradientConic,
    /// Color-matrix image filter shader (`feColorMatrix` / CSS color functions).
    FilterImageColorMatrix,
    /// Fill two-point (independently centered) radial gradient shader.
    ///
    /// This is the general Canvas `createRadialGradient(x0, y0, r0, x1, y1, r1)`
    /// form, where the start and end circles may have different centers. Ordinary
    /// concentric radial gradients keep using the cheaper box-gradient
    /// [`FillGradient`](Self::FillGradient) path.
    FillGradientTwoPointRadial,
    /// Fill image two-point radial gradient shader (multi-stop LUT variant).
    FillImageGradientTwoPointRadial,
    /// Turbulence generator shader (SVG `feTurbulence`); samples the noise
    /// lattice bound in place of the image.
    FilterImageTurbulence,
    /// sRGB transfer-curve shader: linearRGB to sRGB, or the reverse.
    FilterImageTransfer,
    /// Blend shader (SVG `feBlend`): the image over the backdrop bound in the
    /// glyph-texture slot, with one of the sixteen blend modes.
    FilterImageBlend,
    /// Morphology shader (SVG `feMorphology`), one axis per draw like the
    /// blur: the per-channel maximum or minimum within a radius.
    FilterImageMorphology,
    /// Offset shader (SVG `feOffset`): the image shifted by a pixel offset.
    FilterImageOffset,
    /// Blur shader: a fill whose color is the image blurred along one axis
    /// at the position the paint matrix maps the fragment to, its taps
    /// clamped at the image's edges. A blur draws with it onto and from the
    /// render target.
    FillBlur,
}

impl ShaderType {
    /// Convert the shader type to a u8 value.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::FillGradient => 0,
            Self::FillImage => 1,
            Self::Stencil => 2,
            Self::FillImageGradient => 3,
            Self::FilterImage => 4,
            Self::FillColor => 5,
            Self::TextureCopyUnclipped => 6,
            Self::FillColorUnclipped => 7,
            Self::FillGradientConic => 8,
            Self::FillImageGradientConic => 9,
            Self::FilterImageColorMatrix => 10,
            Self::FillGradientTwoPointRadial => 11,
            Self::FillImageGradientTwoPointRadial => 12,
            Self::FilterImageTurbulence => 13,
            Self::FilterImageTransfer => 14,
            Self::FilterImageBlend => 15,
            Self::FilterImageMorphology => 16,
            Self::FilterImageOffset => 17,
            Self::FillBlur => 18,
        }
    }

    /// Convert the shader type to a f32 value.
    pub fn to_f32(self) -> f32 {
        self.to_u8() as f32
    }
}

/// The largest standard deviation one Gaussian blur pass renders. The
/// fragment shader's blur loop is bounded at 24 taps per side (GLES 2.0 needs
/// a constant loop bound) and the kernel reaches 3 sigma, so a pass covers
/// sigma 8 exactly and no more. A blur above it is not clamped away: the chain
/// planner (`blur_passes` in filters.rs) halves the image until the sigma
/// fits one pass and scales the result back up. The coefficients below and
/// the shader's tap count agree on this value.
pub(crate) const MAX_BLUR_SIGMA: f32 = 8.0;

/// Gaussian blur coefficients for `sigma`, sanitized the same way for every
/// backend. Sigma 0 (or negative / NaN) would divide the coefficient by zero
/// and blank the output instead of passing the image through, and a sigma
/// above [`MAX_BLUR_SIGMA`] must clamp to the bound the fragment shader's loop
/// uses so the coefficients and the iteration count agree - a single
/// `filter_image` pass renders such a sigma at the bound; a chain, a layer
/// filter or a shadow blur splits it into passes first and never sends one
/// above it. Near-zero renders as a visually exact copy. Returns the three
/// coefficients and the sanitized sigma the shader must be given.
pub(crate) fn gaussian_blur_coefficients(sigma: f32) -> ([f32; 3], f32) {
    let sigma = if sigma > 0.0 { sigma.min(MAX_BLUR_SIGMA) } else { 1e-3 };
    let x = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
    let y = f32::exp(-0.5 / (sigma * sigma));
    ([x, y, y * y], sigma)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_indices_take_each_fills_fans_in_turn() {
        let with_fills = |cmd_type, fills: &[(usize, usize)]| {
            let mut command = Command::new(cmd_type);
            command.drawables = fills
                .iter()
                .map(|&fill| Drawable {
                    fill_verts: Some(fill),
                    ..Drawable::default()
                })
                .collect();
            command
        };
        let commands = [
            with_fills(
                CommandType::ConvexFill {
                    params: Params::default(),
                },
                &[(4, 4)],
            ),
            // A clip's fill vertices are triangles already.
            with_fills(CommandType::ClipFill, &[(20, 6)]),
            with_fills(
                CommandType::ConcaveFill {
                    stencil_params: Params::default(),
                    fill_params: Params::default(),
                },
                &[(10, 3), (13, 5)],
            ),
        ];

        let mut fans = FanIndices::default();
        fans.build(&commands);

        assert_eq!(
            fans.indices(),
            [4, 5, 6, 4, 6, 7, 10, 11, 12, 13, 14, 15, 13, 15, 16, 13, 16, 17]
        );
        assert_eq!(fans.firsts, [0, 6, 6]);
        assert_eq!(FanIndices::count(5), 9);
    }
}
