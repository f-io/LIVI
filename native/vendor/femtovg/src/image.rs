use bitflags::bitflags;
use imgref::*;
use rgb::alt::Gray;
use rgb::*;
use slotmap::{DefaultKey, SlotMap};

use crate::geometry::Transform2D;
use crate::renderer::ShaderType;

#[cfg(feature = "image-loading")]
use ::image::DynamicImage;

#[cfg(feature = "image-loading")]
use std::convert::TryFrom;

use crate::{ErrorKind, Renderer};

/// An image handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(DefaultKey);

/// Specifies the format of an image's pixels.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PixelFormat {
    /// 24-bit RGB image format (8 bits per channel)
    Rgb8,
    /// 32-bit RGBA image format (8 bits per channel, including alpha)
    Rgba8,
    /// 8-bit grayscale image format
    Gray8,
}

bitflags! {
    /// Represents a set of flags that modify the behavior of an image.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ImageFlags: u32 {
        /// Generates mipmaps when the image is created and after each update;
        /// a minified draw then samples them half a level toward the larger one.
        const GENERATE_MIPMAPS = 1;
        /// Repeats the image in the X direction when rendered.
        const REPEAT_X = 1 << 1;
        /// Repeats the image in the Y direction when rendered.
        const REPEAT_Y = 1 << 2;
        /// Flips (inverses) the image in the Y direction when rendered.
        const FLIP_Y = 1 << 3;
        /// Indicates that the image data has premultiplied alpha.
        const PREMULTIPLIED = 1 << 4;
        /// Uses nearest-neighbor interpolation instead of linear interpolation when rendering the image.
        const NEAREST = 1 << 5;
    }
}

/// Represents the source of an image.
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub enum ImageSource<'a> {
    /// Image source with RGB image format (8 bits per channel)
    Rgb(ImgRef<'a, RGB8>),
    /// Image source with RGBA image format (8 bits per channel, including alpha)
    Rgba(ImgRef<'a, RGBA8>),
    /// Image source with 8-bit grayscale image format
    Gray(ImgRef<'a, Gray<u8>>),
    /// Image source referencing a HTML image element (only available on `wasm32-unknown-unknown`)
    #[cfg(wasm_unknown)]
    HtmlImageElement(&'a web_sys::HtmlImageElement),
    /// Image source referencing a HTML canvas element (only available on `wasm32-unknown-unknown`)
    #[cfg(wasm_unknown)]
    HtmlCanvasElement(&'a web_sys::HtmlCanvasElement),
}

impl ImageSource<'_> {
    /// Returns the format of the image source.
    pub fn format(&self) -> PixelFormat {
        match self {
            Self::Rgb(_) => PixelFormat::Rgb8,
            Self::Rgba(_) => PixelFormat::Rgba8,
            Self::Gray(_) => PixelFormat::Gray8,
            #[cfg(wasm_unknown)]
            Self::HtmlImageElement(_) | Self::HtmlCanvasElement(_) => PixelFormat::Rgba8,
        }
    }

    /// Returns the dimensions (width and height) of the image source.
    pub fn dimensions(&self) -> Size {
        match self {
            Self::Rgb(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Rgba(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Gray(imgref) => Size::new(imgref.width(), imgref.height()),
            #[cfg(wasm_unknown)]
            Self::HtmlImageElement(element) => Size::new(element.width() as usize, element.height() as usize),
            #[cfg(wasm_unknown)]
            Self::HtmlCanvasElement(element) => Size::new(element.width() as usize, element.height() as usize),
        }
    }

    /// Checks that this source may be copied into the image described by `info`
    /// with its top left corner at (`x`, `y`).
    ///
    /// Every [`Renderer::update_image`](crate::Renderer::update_image)
    /// implementation should call this before touching the graphics API. A copy
    /// that reaches past the destination, or that carries a pixel format the
    /// destination was not created with, is a caller mistake and has to be
    /// reported as [`ErrorKind::ImageUpdateOutOfBounds`] or
    /// [`ErrorKind::ImageUpdateWithDifferentFormat`]. Passing one down to the
    /// graphics API instead produces a driver side validation failure, which
    /// backends are generally not able to turn back into a recoverable error:
    /// the usual outcome is an abort that takes the whole process, or on wasm
    /// the whole application, rather than the single failed call.
    pub fn check_update(&self, info: &ImageInfo, x: usize, y: usize) -> Result<(), ErrorKind> {
        let size = self.dimensions();

        // Saturating, so that an origin close to `usize::MAX` reports the error
        // rather than wrapping into a range that looks valid.
        if x.saturating_add(size.width) > info.width() || y.saturating_add(size.height) > info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if info.format() != self.format() {
            return Err(ErrorKind::ImageUpdateWithDifferentFormat);
        }

        Ok(())
    }
}

impl<'a> From<ImgRef<'a, RGB8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGB8>) -> Self {
        Self::Rgb(src)
    }
}

impl<'a> From<ImgRef<'a, RGBA8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGBA8>) -> Self {
        Self::Rgba(src)
    }
}

impl<'a> From<ImgRef<'a, Gray<u8>>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, Gray<u8>>) -> Self {
        Self::Gray(src)
    }
}

#[cfg(wasm_unknown)]
impl<'a> From<&'a web_sys::HtmlImageElement> for ImageSource<'a> {
    fn from(src: &'a web_sys::HtmlImageElement) -> Self {
        Self::HtmlImageElement(src)
    }
}

#[cfg(wasm_unknown)]
impl<'a> From<&'a web_sys::HtmlCanvasElement> for ImageSource<'a> {
    fn from(src: &'a web_sys::HtmlCanvasElement) -> Self {
        Self::HtmlCanvasElement(src)
    }
}

#[cfg(feature = "image-loading")]
impl<'a> TryFrom<&'a DynamicImage> for ImageSource<'a> {
    type Error = ErrorKind;

    fn try_from(src: &'a DynamicImage) -> Result<Self, ErrorKind> {
        Ok(match src {
            ::image::DynamicImage::ImageLuma8(img) => {
                let src: Img<&[Gray<u8>]> = Img::new(img.as_pixels(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgb8(img) => {
                let src = Img::new(img.as_rgb(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgba8(img) => {
                let src = Img::new(img.as_rgba(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            // TODO: if format is not supported maybe we should convert it here,
            // But that is an expensive operation on the render thread that will remain hidden from the user
            _ => return Err(ErrorKind::UnsupportedImageFormat),
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Information about an image.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImageInfo {
    flags: ImageFlags,
    size: Size,
    format: PixelFormat,
}

impl ImageInfo {
    /// Creates a new `ImageInfo` with the specified flags, width, height, and format.
    pub fn new(flags: ImageFlags, width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            flags,
            size: Size { width, height },
            format,
        }
    }

    /// Returns the image flags.
    pub fn flags(&self) -> ImageFlags {
        self.flags
    }

    /// Returns the image width in pixels.
    pub fn width(&self) -> usize {
        self.size.width
    }

    /// Returns the image height in pixels.
    pub fn height(&self) -> usize {
        self.size.height
    }

    /// Returns the image size (width and height) in pixels.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the image format.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Sets the image format.
    pub fn set_format(&mut self, format: PixelFormat) {
        self.format = format;
    }
}

#[derive(Debug)]
pub struct ImageStore<T>(SlotMap<DefaultKey, (ImageInfo, T)>);

impl<T> Default for ImageStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ImageStore<T> {
    pub fn new() -> Self {
        Self(SlotMap::new())
    }

    pub fn alloc<R: Renderer<Image = T>>(&mut self, renderer: &mut R, info: ImageInfo) -> Result<ImageId, ErrorKind> {
        let image = renderer.alloc_image(info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    pub fn register_native_texture<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        texture: R::NativeTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        let image = renderer.create_image_from_native_texture(texture, info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    pub fn register_external_texture<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        texture: R::ExternalTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        let image = renderer.create_image_from_external_texture(texture, info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    // Reallocates the image without changing the id.
    pub fn realloc<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        info: ImageInfo,
    ) -> Result<(), ErrorKind> {
        if let Some(old) = self.0.get_mut(id.0) {
            let new = renderer.alloc_image(info)?;
            let previous = std::mem::replace(&mut old.1, new);
            old.0 = info;
            renderer.delete_image(previous, id);
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn get(&self, id: ImageId) -> Option<&T> {
        self.0.get(id.0).map(|inner| &inner.1)
    }

    pub fn get_mut(&mut self, id: ImageId) -> Option<&mut T> {
        self.0.get_mut(id.0).map(|inner| &mut inner.1)
    }

    pub fn update<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        if let Some(image) = self.0.get_mut(id.0) {
            renderer.update_image(&mut image.1, data, x, y)?;
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn info(&self, id: ImageId) -> Option<ImageInfo> {
        self.0.get(id.0).map(|inner| inner.0)
    }

    pub fn remove<R: Renderer<Image = T>>(&mut self, renderer: &mut R, id: ImageId) {
        if let Some(image) = self.0.remove(id.0) {
            renderer.delete_image(image.1, id);
        }
    }

    pub fn clear<R: Renderer<Image = T>>(&mut self, renderer: &mut R) {
        for (idx, image) in self.0.drain() {
            renderer.delete_image(image.1, ImageId(idx));
        }
    }
}

/// How [`ImageFilter::Blend`] combines a pixel of the image with the
/// backdrop under it: the blend modes of the W3C Compositing and Blending
/// specification, which SVG `feBlend`, CSS `mix-blend-mode` and Canvas 2D
/// `globalCompositeOperation` share. The first twelve apply per color
/// channel; the next four work on hue, saturation and luminosity as a whole.
/// [`PlusLighter`](Self::PlusLighter) is the Compositing 2 operator CSS
/// `mix-blend-mode` and Canvas 2D also accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    /// The source color: `feBlend`'s default.
    Normal,
    /// `Cb * Cs`.
    Multiply,
    /// `Cb + Cs - Cb * Cs`.
    Screen,
    /// Multiplies or screens depending on the backdrop: hard light with the
    /// two swapped.
    Overlay,
    /// The darker of the two.
    Darken,
    /// The lighter of the two.
    Lighten,
    /// Brightens the backdrop to reflect the source.
    ColorDodge,
    /// Darkens the backdrop to reflect the source.
    ColorBurn,
    /// Multiplies or screens depending on the source.
    HardLight,
    /// Darkens or lightens depending on the source, gently.
    SoftLight,
    /// `|Cb - Cs|`.
    Difference,
    /// `Cb + Cs - 2 * Cb * Cs`.
    Exclusion,
    /// The source's hue with the backdrop's saturation and luminosity.
    Hue,
    /// The source's saturation with the backdrop's hue and luminosity.
    Saturation,
    /// The source's hue and saturation with the backdrop's luminosity.
    Color,
    /// The source's luminosity with the backdrop's hue and saturation.
    Luminosity,
    /// `min(1, Cs + Cb)` on premultiplied color and alpha: Compositing 2's
    /// `plus-lighter`, the same addition as [`CompositeOperation::Lighter`](crate::CompositeOperation::Lighter).
    /// A layer with it composites with that operation and reads no backdrop.
    PlusLighter,
}

impl BlendMode {
    /// The mode's index in the shaders' blend function, in declaration order.
    pub(crate) fn shader_index(self) -> f32 {
        self as u8 as f32
    }

    /// Whether a layer with this mode needs the blend pass, which reads the
    /// backdrop under it. `Normal` is plain source-over and `PlusLighter`
    /// the `Lighter` composite operation, so neither does.
    pub(crate) fn reads_backdrop(self) -> bool {
        !matches!(self, Self::Normal | Self::PlusLighter)
    }
}

/// Specifies the type of filter to apply to images with `crate::Canvas::filter_image`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ImageFilter {
    /// Applies a Gaussian blur with a standard deviation per axis: the SVG
    /// `feGaussianBlur` with its two-valued `stdDeviation`, so a motion
    /// streak or a vertical glow blurs along one axis only.
    /// [`gaussian_blur`](Self::gaussian_blur) is the isotropic blur of CSS
    /// and Canvas `blur()`.
    ///
    /// One shader pass covers a standard deviation of at most 8 device
    /// pixels on an axis: `Canvas::filter_image` clamps a larger one to 8,
    /// while `Canvas::filter_image_chain`, a layer filter and a shadow blur
    /// render it at a size halved along that axis until it fits one pass
    /// and scale the result back up, up to a sigma of 512. Taps beyond the
    /// image read transparent.
    GaussianBlur {
        /// The standard deviation along x, in device pixels. Zero, negative
        /// or non-finite values leave that axis unblurred.
        sigma_x: f32,
        /// The standard deviation along y, in device pixels, likewise.
        sigma_y: f32,
    },
    /// Grows (dilate) or shrinks (erode) the opaque regions of an image by
    /// a radius per axis, the SVG `feMorphology` primitive - a design tool's
    /// shadow spread: each output pixel is the per-channel maximum (dilate)
    /// or minimum (erode) of the premultiplied pixels within the radius
    /// along x and along y, a rectangle of `2 rx + 1` by `2 ry + 1` pixels.
    /// The radii are in device pixels and round to whole pixels, as Skia
    /// rounds them (Gecko takes the ceiling); a radius that rounds to zero
    /// leaves that axis alone, and both zero copies the image. Pixels beyond
    /// the image read transparent, so an erosion eats into the border. One
    /// shader pass covers a radius of at most 24 on an axis:
    /// `Canvas::filter_image` clamps a larger one to 24, while a chain and a
    /// layer filter run it as passes whose radii sum to it, which is exact.
    Morphology {
        /// The radius along x, in device pixels.
        radius_x: f32,
        /// The radius along y, in device pixels.
        radius_y: f32,
        /// Whether the opaque regions grow or shrink.
        operator: MorphologyOperator,
    },
    /// Shifts the image by a device-pixel offset, the SVG `feOffset`
    /// primitive: the output at (x, y) is the input at (x - dx, y - dy),
    /// sampled bilinearly, and what shifts in from beyond the image is
    /// transparent. A non-finite offset is zero, and a zero offset is the
    /// copy it describes.
    Offset {
        /// The shift along x, in device pixels; positive moves content right.
        dx: f32,
        /// The shift along y, in device pixels; positive moves content down.
        dy: f32,
    },
    /// Applies a 4x5 color matrix, the operation behind SVG `feColorMatrix` and
    /// the CSS/Canvas `filter` color functions (`grayscale`, `sepia`, ...).
    ///
    /// The 20 values are row-major: the output channel `[r', g', b', a']` is
    /// `M * [r, g, b, a, 1]`, i.e. `r' = m[0]*r + m[1]*g + m[2]*b + m[3]*a +
    /// m[4]`, and so on for rows `m[5..10]`, `m[10..15]`, `m[15..20]`. The matrix
    /// is applied in **unpremultiplied, sRGB** space (matching the CSS filter
    /// functions, which are defined in sRGB) and the result is clamped to
    /// `[0, 1]`, so overflowing matrices cannot produce out-of-range or NaN
    /// pixels. Use the constructors below for the standard CSS functions.
    ColorMatrix {
        /// Row-major 4x5 color matrix.
        matrix: [f32; 20],
    },
    /// Perlin turbulence or fractal noise: the SVG `feTurbulence` primitive,
    /// generated from the reference algorithm in SVG 1.1 section 15.19 that
    /// Chromium and Firefox both implement, so the same parameters give the
    /// same noise as a browser.
    ///
    /// Unlike the other filters this one does not sample its source image -
    /// `feTurbulence` takes no input - it only sizes the output from it.
    /// Each pixel gets four independent noise channels (RGBA, each in
    /// `[0, 1]`) and the result is stored premultiplied. `fractalNoise` is
    /// centered on 0.5 and `turbulence` on 0 in every channel, alpha
    /// included, exactly as the primitive is specified; a chain typically
    /// follows with a [`ColorMatrix`](Self::ColorMatrix) that remaps the
    /// channels into a color and an opacity.
    ///
    /// The noise lives in its own coordinate space: `transform` maps that
    /// space onto output pixels, so the pattern scales and moves with the
    /// content it decorates instead of sticking to the device grid. Pixel
    /// `(x, y)` is evaluated at `transform⁻¹ · (x, y)`; with the identity
    /// transform one noise unit is one pixel, which is what a browser does
    /// at 1:1 and what Firefox does at any zoom (Chromium additionally snaps
    /// the mapped position to whole noise units).
    ///
    /// The primitive's values are defined in the filter's working color
    /// space, which SVG defaults to linearRGB; end such a chain with
    /// [`LinearRgbToSrgb`](Self::LinearRgbToSrgb) to get what a browser
    /// displays for a default `<filter>`.
    ///
    /// Octaves past the tenth are skipped: octave `n` contributes at most
    /// `1 / 2ⁿ`, so the ones skipped change the sum by less than half of an
    /// 8-bit step, and the bound keeps the per-pixel cost finite on GLES 2.0
    /// hardware, where loops need a constant bound.
    Turbulence {
        /// Base frequency per axis, in cycles per noise unit (SVG `baseFrequency`).
        base_frequency: [f32; 2],
        /// Number of octaves to sum (SVG `numOctaves`); 0 sums nothing.
        num_octaves: u32,
        /// Random seed (SVG `seed`, already truncated to an integer).
        seed: i32,
        /// Adjust the frequency and wrap the lattice so the noise tiles
        /// seamlessly across the output image (SVG `stitchTiles="stitch"`).
        stitch_tiles: bool,
        /// `fractalNoise` or `turbulence` (SVG `type`).
        kind: TurbulenceKind,
        /// Maps noise space onto output pixels.
        transform: Transform2D,
    },
    /// Converts unpremultiplied color from linearRGB to sRGB with the sRGB
    /// transfer curve (IEC 61966-2-1), leaving alpha unchanged. SVG filters
    /// run in linearRGB by default (`color-interpolation-filters`), so a
    /// chain that reproduces one ends with this to hand the display the
    /// sRGB values a browser shows. Adjacent to its inverse it folds away.
    LinearRgbToSrgb,
    /// The inverse of [`LinearRgbToSrgb`](Self::LinearRgbToSrgb): converts
    /// unpremultiplied sRGB color to linearRGB, for filtering an ordinary
    /// image the way a linearRGB SVG filter would.
    SrgbToLinearRgb,
    /// Blends the image over a backdrop image: SVG `feBlend` with the image
    /// as `in` and `backdrop` as `in2`. Both are premultiplied and the result
    /// is the specification's `cs * (1 - ab) + cb * (1 - as) + as * ab *
    /// B(Cb, Cs)`, so where the backdrop is transparent the image passes
    /// through unchanged and where the image is transparent the backdrop
    /// shows. The backdrop is placed at the rect `x, y, width, height` and is
    /// transparent outside it: the rect is in the pixels of the image being
    /// filtered for [`Canvas::filter_image_chain`](crate::Canvas::filter_image_chain),
    /// and in root device space for a layer's filters, as a layer mask's rect
    /// is. A blend is one shader pass over both images plus the draw that
    /// places the backdrop; it needs its own scratch image, reserved with the
    /// chain's.
    Blend {
        /// How the two combine.
        mode: BlendMode,
        /// The image under the filtered one (`in2`).
        backdrop: ImageId,
        /// Left edge of the backdrop's placement.
        x: f32,
        /// Top edge of the backdrop's placement.
        y: f32,
        /// Width of the backdrop's placement.
        width: f32,
        /// Height of the backdrop's placement.
        height: f32,
    },
    /// Clips the image to a rectangle, transparent outside it: the filter
    /// primitive subregion of SVG - by default the filter region - that each
    /// primitive's result is clipped to, and Skia's crop filter. The
    /// rectangle is rounded out to whole pixels; it is in the pixels of the
    /// image being filtered for
    /// [`Canvas::filter_image_chain`](crate::Canvas::filter_image_chain), and
    /// in root device space for a layer's filters, as a blend's backdrop
    /// rect is. After another filter of the chain it costs nothing: that
    /// filter's result is drawn inside the rectangle only.
    Crop {
        /// Left edge of the rectangle.
        x: f32,
        /// Top edge of the rectangle.
        y: f32,
        /// Width of the rectangle.
        width: f32,
        /// Height of the rectangle.
        height: f32,
    },
}

/// The noise function of [`ImageFilter::Turbulence`]: SVG `feTurbulence`'s
/// `type` attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TurbulenceKind {
    /// Sums signed octaves: smooth, cloud-like values centered on 0.5.
    FractalNoise,
    /// Sums the absolute value of each octave: sharper, vein-like values
    /// centered on 0. The SVG default.
    #[default]
    Turbulence,
}

/// What an [`ImageFilter::Morphology`] does to the opaque regions of an
/// image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphologyOperator {
    /// Shrinks them: each pixel takes the minimum within the radius.
    Erode,
    /// Grows them: each pixel takes the maximum within the radius.
    Dilate,
}

impl ImageFilter {
    /// A Gaussian blur of `sigma` on both axes: CSS and Canvas `blur()`,
    /// SVG `feGaussianBlur` with a single `stdDeviation` value.
    pub fn gaussian_blur(sigma: f32) -> Self {
        Self::GaussianBlur {
            sigma_x: sigma,
            sigma_y: sigma,
        }
    }

    /// The identity color matrix (leaves an image unchanged).
    pub const IDENTITY_MATRIX: [f32; 20] = [
        1.0, 0.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];

    // sRGB / Rec.709 luma weights used by the CSS `grayscale`/`saturate`/
    // `hue-rotate` functions (Filter Effects Level 1).
    const LR: f32 = 0.2126;
    const LG: f32 = 0.7152;
    const LB: f32 = 0.0722;

    /// CSS `grayscale(amount)`; `amount` is clamped to `[0, 1]` (1 = fully gray).
    pub fn grayscale(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let inv = 1.0 - a;
        let (lr, lg, lb) = (Self::LR, Self::LG, Self::LB);
        Self::ColorMatrix {
            matrix: [
                lr + 0.7874 * inv,
                lg - lg * inv,
                lb - lb * inv,
                0.0,
                0.0, //
                lr - lr * inv,
                lg + 0.2848 * inv,
                lb - lb * inv,
                0.0,
                0.0, //
                lr - lr * inv,
                lg - lg * inv,
                lb + 0.9278 * inv,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `sepia(amount)`; `amount` is clamped to `[0, 1]`.
    pub fn sepia(amount: f32) -> Self {
        let inv = 1.0 - amount.clamp(0.0, 1.0);
        Self::ColorMatrix {
            matrix: [
                0.393 + 0.607 * inv,
                0.769 - 0.769 * inv,
                0.189 - 0.189 * inv,
                0.0,
                0.0, //
                0.349 - 0.349 * inv,
                0.686 + 0.314 * inv,
                0.168 - 0.168 * inv,
                0.0,
                0.0, //
                0.272 - 0.272 * inv,
                0.534 - 0.534 * inv,
                0.131 + 0.869 * inv,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// The identity color matrix: leaves every pixel unchanged. Chains use it
    /// as an explicit no-op pass; it is exact by construction.
    pub fn identity() -> Self {
        #[rustfmt::skip]
        let matrix = [
            1.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        Self::ColorMatrix { matrix }
    }

    /// CSS `saturate(amount)` (`feColorMatrix type="saturate"`). `amount` >= 0;
    /// 0 desaturates, 1 is identity, >1 over-saturates. Uses the SVG
    /// 0.213/0.715/0.072 luma weights.
    pub fn saturate(amount: f32) -> Self {
        let s = amount.max(0.0);
        let (lr, lg, lb) = (0.213f32, 0.715f32, 0.072f32);
        Self::ColorMatrix {
            matrix: [
                lr + 0.787 * s,
                lg - lg * s,
                lb - lb * s,
                0.0,
                0.0, //
                lr - lr * s,
                lg + 0.285 * s,
                lb - lb * s,
                0.0,
                0.0, //
                lr - lr * s,
                lg - lg * s,
                lb + 0.928 * s,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `hue-rotate(radians)` (`feColorMatrix type="hueRotate"`).
    pub fn hue_rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        let (lr, lg, lb) = (0.213f32, 0.715f32, 0.072f32);
        Self::ColorMatrix {
            matrix: [
                lr + c * 0.787 - s * 0.213,
                lg - c * lg - s * lg,
                lb - c * lb + s * 0.928,
                0.0,
                0.0, //
                lr - c * lr + s * 0.143,
                lg + c * 0.285 + s * 0.140,
                lb - c * lb - s * 0.283,
                0.0,
                0.0, //
                lr - c * lr - s * 0.787,
                lg - c * lg + s * lg,
                lb + c * 0.928 + s * lb,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `brightness(amount)`; `amount` >= 0 scales each RGB channel.
    pub fn brightness(amount: f32) -> Self {
        let a = amount.max(0.0);
        Self::ColorMatrix {
            matrix: [
                a, 0.0, 0.0, 0.0, 0.0, //
                0.0, a, 0.0, 0.0, 0.0, //
                0.0, 0.0, a, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// CSS `contrast(amount)`; `amount` >= 0. 1 is identity.
    pub fn contrast(amount: f32) -> Self {
        let a = amount.max(0.0);
        let b = 0.5 - 0.5 * a;
        Self::ColorMatrix {
            matrix: [
                a, 0.0, 0.0, 0.0, b, //
                0.0, a, 0.0, 0.0, b, //
                0.0, 0.0, a, 0.0, b, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// CSS `invert(amount)`; `amount` is clamped to `[0, 1]`.
    pub fn invert(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let d = 1.0 - 2.0 * a;
        Self::ColorMatrix {
            matrix: [
                d, 0.0, 0.0, 0.0, a, //
                0.0, d, 0.0, 0.0, a, //
                0.0, 0.0, d, 0.0, a, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// The `feColorMatrix type="luminanceToAlpha"` conversion: the output
    /// alpha is the input's luminance and the color channels go to zero,
    /// which is how SVG luminance masks read their content. Uses the Rec.
    /// 709 weights above, the ones Skia's luma filter applies for Chromium's
    /// masks; the SVG row (0.2125, 0.7154, 0.0721) differs by at most
    /// 0.0002. Like every color matrix it runs on straight (unpremultiplied)
    /// color, so the luminance is the color's own, not scaled by alpha.
    pub fn luminance_to_alpha() -> Self {
        #[rustfmt::skip]
        let matrix = [
            0.0,      0.0,      0.0,      0.0, 0.0,
            0.0,      0.0,      0.0,      0.0, 0.0,
            0.0,      0.0,      0.0,      0.0, 0.0,
            Self::LR, Self::LG, Self::LB, 0.0, 0.0,
        ];
        ImageFilter::ColorMatrix { matrix }
    }

    /// CSS `opacity(amount)`; `amount` is clamped to `[0, 1]` and scales alpha.
    pub fn opacity(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        Self::ColorMatrix {
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, a, 0.0,
            ],
        }
    }

    /// Folds this filter with `next` (applied after it) into a single
    /// equivalent filter when both are color matrices and the fold is exact.
    ///
    /// This is the load-bearing rule for filter chains: a run of adjacent
    /// color operations costs one GPU pass and zero intermediate textures,
    /// because 4x5 matrices compose by multiplication on the CPU. Composition
    /// is exact only where it preserves the per-pass clamp the shader applies:
    /// `renderColorMatrix` clamps each result to [0, 1] before the next pass
    /// reads it, and both CSS/SVG and Skia clamp per filter function too (Skia
    /// carries an explicit `Clamp` flag on `SkColorFilters::Matrix`). Folding
    /// drops the clamp between the two matrices, so it matches running them
    /// separately only when `self` - the one that runs first - never leaves
    /// [0, 1] for inputs in [0, 1]. A matrix that can push a channel out of
    /// range (`brightness(>1)`, `contrast`, `sepia`, `saturate(>1)`) is
    /// therefore left as its own pass, so its clamp still happens; without the
    /// guard, `brightness(2)` then `saturate(0)` came out ~33/255 off from the
    /// sequential result and from Chromium. `next` may overflow freely: its
    /// output is clamped at the end of the pass either way.
    ///
    /// A color-space transfer followed by its inverse folds to the identity:
    /// the pair is a mathematical no-op, and skipping it is more exact than
    /// running it, which would round the linear intermediate to 8 bits.
    ///
    /// Returns `None` for any other pairing that is not two color matrices
    /// (a blur or a turbulence cannot fold) or when `self` is not
    /// range-safe, leaving chain execution to run the passes separately.
    pub fn fold_with(self, next: Self) -> Option<Self> {
        if matches!(
            (self, next),
            (Self::LinearRgbToSrgb, Self::SrgbToLinearRgb) | (Self::SrgbToLinearRgb, Self::LinearRgbToSrgb)
        ) {
            return Some(Self::identity());
        }
        let (Self::ColorMatrix { matrix: a }, Self::ColorMatrix { matrix: b }) = (self, next) else {
            return None;
        };
        // Folding drops the clamp between the two matrices; that only matches
        // the two-pass result when the first matrix never needs it.
        if !color_matrix_range_safe(&a) {
            return None;
        }
        // `self` runs first, `next` second: out = B * augment(A), where
        // augment(A) extends the 4x5 matrix with the implicit [0 0 0 0 1] row
        // so the constant column composes correctly.
        let mut m = [0.0f32; 20];
        for row in 0..4 {
            for col in 0..5 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += b[row * 5 + k] * a[k * 5 + col];
                }
                if col == 4 {
                    // The implicit augmented row contributes next's constant.
                    sum += b[row * 5 + 4];
                }
                m[row * 5 + col] = sum;
            }
        }
        Some(Self::ColorMatrix { matrix: m })
    }

    /// How one pass of this filter leaves its result stored, given how its
    /// source is (`true`: the way a render target is, rows bottom up). A
    /// single draw stores the other way up from the texel rows it read; the
    /// two-draw Gaussian blur and morphology turn the image over twice; the
    /// turbulence generator reads nothing, so its noise lands the way any
    /// draw into a target does whatever its source. Exhaustive on purpose -
    /// a new variant must declare itself here.
    pub(crate) fn stores_flipped(&self, source_flipped: bool) -> bool {
        match self {
            Self::Turbulence { .. } => true,
            Self::ColorMatrix { .. }
            | Self::LinearRgbToSrgb
            | Self::SrgbToLinearRgb
            | Self::Blend { .. }
            | Self::Offset { .. }
            | Self::Crop { .. } => !source_flipped,
            Self::GaussianBlur { .. } | Self::Morphology { .. } => source_flipped,
        }
    }

    /// Whether this filter runs as two draws along the axes through a
    /// horizontal scratch image: the Gaussian blur and the morphology.
    pub(crate) fn two_pass(&self) -> bool {
        matches!(self, Self::GaussianBlur { .. } | Self::Morphology { .. })
    }

    /// This filter as it must run over a source stored the way a render
    /// target is (`source_flipped`, rows bottom up) to mean what it means
    /// over an upright one: a pass samples texel rows as they are stored, so
    /// an offset's `dy` points the other way in such storage; every other
    /// filter is the same either way.
    pub(crate) fn oriented(self, source_flipped: bool) -> Self {
        match self {
            Self::Offset { dx, dy } if source_flipped => Self::Offset { dx, dy: -dy },
            other => other,
        }
    }

    /// The shader and its 20 parameter slots for a filter that runs as one
    /// full-image pass over a `width` x `height` target; `None` for the
    /// two-pass Gaussian blur and morphology, which the renderers drive
    /// themselves.
    ///
    /// The slots ride the scissor and paint matrix uniforms, which are dead
    /// during a filter pass (no scissor, no paint gradient): the first 12
    /// land in `scissor_mat`, the last 8 in `paint_mat`, so no uniform-array
    /// growth is needed. Each shader documents its own layout; the color
    /// matrix is its 20 values row-major, a transfer uses slot 0 as its
    /// direction (1 = to sRGB), turbulence is laid out by
    /// [`turbulence::shader_slots`](crate::turbulence::shader_slots).
    pub(crate) fn single_pass(&self, width: f32, height: f32) -> Option<(ShaderType, [f32; 20])> {
        let transfer = |to_srgb: f32| {
            let mut slots = [0.0f32; 20];
            slots[0] = to_srgb;
            (ShaderType::FilterImageTransfer, slots)
        };
        match *self {
            Self::GaussianBlur { .. } | Self::Morphology { .. } => None,
            Self::ColorMatrix { matrix } => Some((ShaderType::FilterImageColorMatrix, matrix)),
            Self::Offset { dx, dy } => {
                let mut slots = [0.0f32; 20];
                slots[0] = dx;
                slots[1] = dy;
                Some((ShaderType::FilterImageOffset, slots))
            }
            Self::Turbulence { .. } => Some((
                ShaderType::FilterImageTurbulence,
                crate::turbulence::shader_slots(self, width, height),
            )),
            Self::LinearRgbToSrgb => Some(transfer(1.0)),
            Self::SrgbToLinearRgb => Some(transfer(0.0)),
            // A crop with no pass before it to ride is a copy, cropped.
            Self::Crop { .. } => Self::identity().single_pass(width, height),
            Self::Blend { mode, .. } => {
                // Slot 1, whether the backdrop is sampled upside down, is the
                // pass's to set: it depends on where in a chain the blend runs.
                let mut slots = [0.0f32; 20];
                slots[0] = mode.shader_index();
                Some((ShaderType::FilterImageBlend, slots))
            }
        }
    }
}

/// Whether a 4x5 color matrix maps every input in [0, 1] to an output in
/// [0, 1], i.e. the shader's per-pass clamp would be a no-op on its result.
/// Only such a matrix is safe to fold into a following pass (see
/// [`ImageFilter::fold_with`]); a matrix that can overflow must keep its own
/// clamped pass.
///
/// Each output channel is affine in the four inputs, so its extremes over the
/// unit box `[0, 1]^4` are at the corners: the maximum is `constant + sum of
/// positive coefficients`, the minimum is `constant + sum of negative
/// coefficients`. The matrix is range-safe when every row's maximum is `<= 1`
/// and minimum is `>= 0`. A small epsilon keeps rows that sum to exactly 1.0
/// (saturate, grayscale) foldable despite f32 rounding; a non-finite
/// coefficient is treated as unsafe so it never folds.
pub(crate) fn color_matrix_range_safe(matrix: &[f32; 20]) -> bool {
    const EPS: f32 = 1e-4;
    for row in 0..4 {
        let mut max = matrix[row * 5 + 4];
        let mut min = matrix[row * 5 + 4];
        for col in 0..4 {
            let c = matrix[row * 5 + col];
            if !c.is_finite() {
                return false;
            }
            if c > 0.0 {
                max += c;
            } else {
                min += c;
            }
        }
        if !max.is_finite() || !min.is_finite() || max > 1.0 + EPS || min < -EPS {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod filter_fold_tests {
    use super::{color_matrix_range_safe, ImageFilter};

    fn apply(m: &[f32; 20], px: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for row in 0..4 {
            out[row] = m[row * 5] * px[0]
                + m[row * 5 + 1] * px[1]
                + m[row * 5 + 2] * px[2]
                + m[row * 5 + 3] * px[3]
                + m[row * 5 + 4];
        }
        out
    }

    /// Models one shader pass: apply the matrix, then clamp to [0, 1] the way
    /// `renderColorMatrix` does before the next pass reads the result.
    fn apply_clamped(m: &[f32; 20], px: [f32; 4]) -> [f32; 4] {
        let mut out = apply(m, px);
        for c in &mut out {
            *c = c.clamp(0.0, 1.0);
        }
        out
    }

    fn matrix(f: &ImageFilter) -> [f32; 20] {
        let ImageFilter::ColorMatrix { matrix } = f else {
            panic!("not a color matrix")
        };
        *matrix
    }

    /// When the first matrix is range-safe the fold is exact: it equals the
    /// two-pass result whether or not the intermediate clamp fires (it never
    /// does), which is the property that lets a range-safe color run collapse
    /// to one GPU pass.
    #[test]
    fn folding_matches_sequential_application() {
        // Every first matrix here is range-safe (maps [0,1] into [0,1]).
        let pairs = [
            (ImageFilter::saturate(0.5), ImageFilter::hue_rotate(1.1)),
            (ImageFilter::grayscale(0.7), ImageFilter::invert(1.0)),
            (ImageFilter::invert(1.0), ImageFilter::sepia(0.8)),
            (ImageFilter::opacity(0.6), ImageFilter::brightness(2.0)),
        ];
        for (first, second) in pairs {
            assert!(
                color_matrix_range_safe(&matrix(&first)),
                "first matrix should be range-safe"
            );
            let folded = first.fold_with(second).expect("range-safe first folds");
            for px in [
                [1.0, 0.0, 0.0, 1.0],
                [0.2, 0.7, 0.4, 0.5],
                [0.0, 0.0, 0.0, 0.0],
                [1.0, 1.0, 1.0, 1.0],
                [0.9, 0.1, 0.6, 0.3],
            ] {
                // A range-safe first matrix never overflows, so its own pass
                // clamp is a no-op - this is exactly what makes the fold exact.
                let first_raw = apply(&matrix(&first), px);
                let first_clamped = apply_clamped(&matrix(&first), px);
                for c in 0..4 {
                    assert!(
                        (first_raw[c] - first_clamped[c]).abs() < 1e-5,
                        "range-safe first should stay within [0,1]"
                    );
                }
                // One clamped pass (the fold) equals two clamped passes. The
                // second matrix may overflow; both sides clamp it at the end.
                let two_pass = apply_clamped(&matrix(&second), first_clamped);
                let one_pass = apply_clamped(&matrix(&folded), px);
                for c in 0..4 {
                    assert!(
                        (two_pass[c] - one_pass[c]).abs() < 1e-5,
                        "channel {c} of {px:?}: two-pass {} vs folded {}",
                        two_pass[c],
                        one_pass[c]
                    );
                }
            }
        }
    }

    /// A first matrix that can leave [0, 1] must not fold: the fold would drop
    /// the clamp that separates the passes. `brightness(2)` then `saturate(0)`
    /// is the witness - folded and sequential luma differ by ~0.13.
    #[test]
    fn fold_guard_rejects_range_unsafe_first() {
        for unsafe_first in [
            ImageFilter::brightness(2.0),
            ImageFilter::contrast(2.0),
            ImageFilter::sepia(1.0),
            ImageFilter::saturate(2.0),
        ] {
            assert!(
                !color_matrix_range_safe(&matrix(&unsafe_first)),
                "matrix should be flagged range-unsafe"
            );
            assert!(
                unsafe_first.fold_with(ImageFilter::saturate(0.0)).is_none(),
                "a range-unsafe first matrix must not fold"
            );
        }

        // The divergence the guard prevents, computed on rgb (0.8, 0.1, 0.1):
        // folded would grayscale the unclamped (1.6, 0.2, 0.2); the passes
        // grayscale the clamped (1.0, 0.2, 0.2). Confirm they really differ.
        let px = [0.8, 0.1, 0.1, 1.0];
        let bright = matrix(&ImageFilter::brightness(2.0));
        let gray = matrix(&ImageFilter::saturate(0.0));
        let sequential = apply_clamped(&gray, apply_clamped(&bright, px));
        let would_be_folded = apply(&gray, apply(&bright, px));
        assert!(
            (sequential[0] - would_be_folded[0]).abs() > 0.1,
            "sequential {} should differ from an unclamped fold {}",
            sequential[0],
            would_be_folded[0]
        );
    }

    /// Folding with an identity leaves the other matrix unchanged, and the
    /// constant column composes in order. `brightness(0.5)` and `invert(1.0)`
    /// are both range-safe, so both directions fold, and they do not commute:
    /// brighten-then-invert is `1 - 0.5c`, invert-then-brighten is `0.5 - 0.5c`.
    /// luminanceToAlpha writes the Rec. 709 luminance into alpha and nothing
    /// into the color rows, so white converts to full coverage and a pure
    /// green pixel to 0.7152 of it.
    #[test]
    fn luminance_to_alpha_moves_luminance_into_alpha_only() {
        let ImageFilter::ColorMatrix { matrix } = ImageFilter::luminance_to_alpha() else {
            panic!("luminance_to_alpha is a color matrix");
        };
        assert!(matrix[..15].iter().all(|c| *c == 0.0), "color rows are zero");
        assert_eq!(&matrix[15..], &[0.2126, 0.7152, 0.0722, 0.0, 0.0]);
        let white: f32 = matrix[15..18].iter().sum();
        assert!((white - 1.0).abs() < 1e-4, "white is full coverage, got {white}");
    }

    #[test]
    fn folding_respects_order_and_identity() {
        let bright = ImageFilter::brightness(0.5);
        let invert = ImageFilter::invert(1.0);
        let bi = matrix(&bright.fold_with(invert).unwrap());
        let ib = matrix(&invert.fold_with(bright).unwrap());
        let px = [0.25, 0.5, 0.75, 1.0];
        let ab = apply(&bi, px);
        let ba = apply(&ib, px);
        assert!(
            (ab[0] - (1.0 - 0.5 * 0.25)).abs() < 1e-5,
            "brighten-then-invert got {}",
            ab[0]
        );
        assert!(
            (ba[0] - (0.5 - 0.5 * 0.25)).abs() < 1e-5,
            "invert-then-brighten got {}",
            ba[0]
        );

        // identity as the first pass is range-safe, so it always folds and
        // leaves the following matrix untouched.
        let identity = ImageFilter::identity();
        let folded = identity.fold_with(ImageFilter::sepia(1.0)).unwrap();
        let direct = matrix(&ImageFilter::sepia(1.0));
        for (x, y) in matrix(&folded).iter().zip(direct.iter()) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    /// Blurs cannot fold - the chain executor must run them as passes.
    #[test]
    fn blur_does_not_fold() {
        let blur = ImageFilter::gaussian_blur(2.0);
        assert!(blur.fold_with(ImageFilter::sepia(1.0)).is_none());
        assert!(ImageFilter::sepia(1.0).fold_with(blur).is_none());
    }
}
