use std::rc::Rc;

use glow::HasContext;

use crate::{ErrorKind, ImageFlags, ImageInfo, ImageSource, PixelFormat};

#[derive(Debug)]
pub struct GlTexture {
    id: <glow::Context as glow::HasContext>::Texture,
    info: ImageInfo,
    owned: bool,
    pub(super) external: bool,
}

impl GlTexture {
    pub fn new_from_native_texture(
        texture: <glow::Context as glow::HasContext>::Texture,
        info: ImageInfo,
        external: bool,
    ) -> Self {
        Self {
            id: texture,
            info,
            owned: false,
            external,
        }
    }
    pub fn new(context: &Rc<glow::Context>, info: ImageInfo, opengles_2_0: bool) -> Result<Self, ErrorKind> {
        //let size = src.dimensions();

        let id = unsafe {
            let id = context.create_texture().unwrap();
            context.bind_texture(glow::TEXTURE_2D, Some(id));
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, info.width() as i32);
                context.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            }
            id
        };

        let texture = Self {
            id,
            info,
            owned: true,
            external: false,
        };

        // Upload a zeroed buffer instead of NULL so the texture's contents are
        // defined from creation: glTexImage2D with NULL leaves them undefined,
        // and embedded GL drivers hand back recycled, unscrubbed memory that
        // edge-of-quad NEAREST lookups can pick up as stray pixels (#310).
        // Desktop drivers zero pages in practice, which is why this went
        // unnoticed; the wgpu backend guarantees zero-initialized textures, so
        // this also aligns the two backends.
        let bytes_per_pixel = match info.format() {
            PixelFormat::Gray8 => 1,
            PixelFormat::Rgb8 => 3,
            PixelFormat::Rgba8 => 4,
        };
        let zeroed = vec![0u8; info.width() * info.height() * bytes_per_pixel];

        match info.format() {
            PixelFormat::Gray8 => unsafe {
                let internal_format = if opengles_2_0 { glow::LUMINANCE } else { glow::R8 };
                let format = if opengles_2_0 { internal_format } else { glow::RED };

                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    internal_format as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&zeroed)),
                );
            },
            PixelFormat::Rgb8 => unsafe {
                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGB as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&zeroed)),
                );
            },
            PixelFormat::Rgba8 => unsafe {
                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&zeroed)),
                );
            },
        }

        let flags = texture.info.flags();

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe {
                    context.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::NEAREST_MIPMAP_NEAREST as i32,
                    );
                }
            } else {
                unsafe {
                    context.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::LINEAR_MIPMAP_LINEAR as i32,
                    );
                }
            }
        } else if flags.contains(ImageFlags::NEAREST) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            }
        }

        if flags.contains(ImageFlags::NEAREST) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_X) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_Y) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
        }

        unsafe {
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            }
        }

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                context.generate_mipmap(glow::TEXTURE_2D);
                //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
            }
        }

        unsafe {
            context.bind_texture(glow::TEXTURE_2D, None);
        }

        Ok(texture)
    }

    pub fn id(&self) -> <glow::Context as glow::HasContext>::Texture {
        self.id
    }

    pub fn update(
        &mut self,
        context: &Rc<glow::Context>,
        src: ImageSource,
        x: usize,
        y: usize,
        opengles_2_0: bool,
    ) -> Result<(), ErrorKind> {
        let size = src.dimensions();

        src.check_update(&self.info, x, y)?;

        unsafe {
            context.bind_texture(glow::TEXTURE_2D, Some(self.id));
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, size.width as i32);
            }
        }

        match src {
            ImageSource::Gray(data) => unsafe {
                let format = if opengles_2_0 { glow::LUMINANCE } else { glow::RED };

                context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.width as i32,
                    size.height as i32,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(data.buf().align_to().1)),
                );
            },
            ImageSource::Rgb(data) => unsafe {
                context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.width as i32,
                    size.height as i32,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(data.buf().align_to().1)),
                );
            },
            ImageSource::Rgba(data) => unsafe {
                context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.width as i32,
                    size.height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(data.buf().align_to().1)),
                );
            },
            #[cfg(target_arch = "wasm32")]
            ImageSource::HtmlImageElement(image_element) => unsafe {
                context.tex_sub_image_2d_with_html_image(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    image_element,
                )
            },
            #[cfg(target_arch = "wasm32")]
            ImageSource::HtmlCanvasElement(canvas_element) => unsafe {
                context.tex_sub_image_2d_with_html_canvas(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    canvas_element,
                )
            },
        }

        if self.info.flags().contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                context.generate_mipmap(glow::TEXTURE_2D);
                //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
            }
        }

        unsafe {
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            }
            //glow::PixelStorei(glow::UNPACK_SKIP_PIXELS, 0);
            //glow::PixelStorei(glow::UNPACK_SKIP_ROWS, 0);
            context.bind_texture(glow::TEXTURE_2D, None);
        }

        Ok(())
    }

    pub fn delete(self, context: &Rc<glow::Context>) {
        if self.owned {
            unsafe {
                context.delete_texture(self.id);
            }
        }
    }

    pub fn info(&self) -> ImageInfo {
        self.info
    }
}
