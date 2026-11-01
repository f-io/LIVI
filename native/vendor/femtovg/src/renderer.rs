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
pub use wgpu::{WGPURenderOutput, WGPURenderer};

mod void;
pub use void::Void;

mod params;
pub(crate) use params::Params;

/// Represents a drawable object.
#[derive(Copy, Clone, Default, Debug)]
pub struct Drawable {
    pub(crate) fill_verts: Option<(usize, usize)>,
    pub(crate) stroke_verts: Option<(usize, usize)>,
}

/// Defines different types of commands that can be executed by the renderer.
#[derive(Debug)]
pub enum CommandType {
    /// Set the render target (screen or image).
    SetRenderTarget(RenderTarget),
    /// Clear a rectangle with the specified color.
    ClearRect {
        /// Color to fill the rectangle with.
        color: Color,
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
}

/// Represents a command that can be executed by the renderer.
#[derive(Debug)]
pub struct Command {
    pub(crate) cmd_type: CommandType,
    pub(crate) drawables: Vec<Drawable>,
    pub(crate) triangles_verts: Option<(usize, usize)>,
    pub(crate) image: Option<ImageId>,
    pub(crate) glyph_texture: GlyphTexture,
    pub(crate) fill_rule: FillRule,
    pub(crate) composite_operation: CompositeOperationState,
}

impl Command {
    /// Creates a new command with the specified command type.
    pub fn new(flavor: CommandType) -> Self {
        Self {
            cmd_type: flavor,
            drawables: Vec::new(),
            triangles_verts: None,
            image: None,
            glyph_texture: GlyphTexture::default(),
            fill_rule: FillRule::default(),
            composite_operation: CompositeOperationState::default(),
        }
    }
}

/// Represents different render targets (screen or image).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
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
        }
    }

    /// Convert the shader type to a f32 value.
    pub fn to_f32(self) -> f32 {
        self.to_u8() as f32
    }
}
