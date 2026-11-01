use std::{
    error::Error,
    ffi::NulError,
    fmt::{self, Display, Formatter},
    io,
};

/// Enum representing different types of errors that can occur in the canvas.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// An unknown error occurred.
    UnknownError,
    /// A general error with a custom error message.
    GeneralError(String),
    /// An error related to image loading (requires "image-loading" feature).
    #[cfg(feature = "image-loading")]
    ImageError(::image::ImageError),
    /// An I/O error.
    IoError(io::Error),
    /// An error occurred while parsing a font.
    FontParseError,
    /// No font was found.
    NoFontFound,
    /// An error occurred while extracting font information.
    FontInfoExtractionError,
    /// The font size is too large for the font atlas.
    FontSizeTooLargeForAtlas,
    /// An error occurred while compiling a shader.
    ShaderCompileError(String),
    /// An error occurred while linking a shader.
    ShaderLinkError(String),
    /// An error related to a render target.
    RenderTargetError(String),
    /// The specified image ID was not found.
    ImageIdNotFound,
    /// An error occurred while updating an image, as it is out of bounds.
    ImageUpdateOutOfBounds,
    /// An error occurred while updating an image with a different format.
    ImageUpdateWithDifferentFormat,
    /// The specified image format is not supported.
    UnsupportedImageFormat,
    /// The requested operation is not supported (for example screenshot by wgpu renderer).
    UnsupportedOperation,
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

#[cfg(feature = "image-loading")]
impl From<::image::ImageError> for ErrorKind {
    fn from(error: ::image::ImageError) -> Self {
        Self::ImageError(error)
    }
}

impl From<io::Error> for ErrorKind {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

impl From<NulError> for ErrorKind {
    fn from(error: NulError) -> Self {
        Self::GeneralError(error.to_string())
    }
}

impl Error for ErrorKind {}
