use std::rc::Rc;

use glow::HasContext;

use crate::ErrorKind;

use super::GlTexture;

#[derive(Debug)]
pub struct Framebuffer {
    context: Rc<glow::Context>,
    fbo: <glow::Context as glow::HasContext>::Framebuffer,
    stencil_rbo: Option<<glow::Context as glow::HasContext>::Renderbuffer>,
}

impl Framebuffer {
    pub fn from_external(context: &Rc<glow::Context>, fbo: <glow::Context as glow::HasContext>::Framebuffer) -> Self {
        Self {
            context: context.clone(),
            fbo,
            stencil_rbo: None,
        }
    }
    pub fn new(context: &Rc<glow::Context>, texture: &GlTexture) -> Result<Self, ErrorKind> {
        let fbo = unsafe { context.create_framebuffer().unwrap() };
        unsafe {
            context.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        }

        let width = texture.info().width() as u32;
        let height = texture.info().height() as u32;

        unsafe {
            context.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture.id()),
                0,
            );
        };

        let stencil_rbo = unsafe { context.create_renderbuffer().unwrap() };
        unsafe {
            context.bind_renderbuffer(glow::RENDERBUFFER, Some(stencil_rbo));
            context.renderbuffer_storage(glow::RENDERBUFFER, glow::STENCIL_INDEX8, width as i32, height as i32);
            context.bind_renderbuffer(glow::RENDERBUFFER, None);

            context.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::STENCIL_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(stencil_rbo),
            );

            let status = context.check_framebuffer_status(glow::FRAMEBUFFER);

            if status != glow::FRAMEBUFFER_COMPLETE {
                let reason = match status {
                    glow::FRAMEBUFFER_INCOMPLETE_ATTACHMENT => {
                        format!("({status}) Framebuffer incomplete attachment")
                    }
                    //glow::FRAMEBUFFER_INCOMPLETE_DRAW_BUFFER => format!("({}) Framebuffer incomplete draw buffer", status),
                    //glow::FRAMEBUFFER_INCOMPLETE_LAYER_TARGETS => format!("({}) Framebuffer incomplete layer targets", status),
                    //FIXME: will be in next glow release: glow::FRAMEBUFFER_INCOMPLETE_DIMENSIONS => format!("({}) Framebuffer incomplete dimensions", status),
                    glow::FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT => {
                        format!("({status}) Framebuffer incomplete missing attachment")
                    }
                    glow::FRAMEBUFFER_INCOMPLETE_MULTISAMPLE => {
                        format!("({status}) Framebuffer incomplete multisample")
                    }
                    //glow::FRAMEBUFFER_INCOMPLETE_READ_BUFFER => format!("({}) Framebuffer incomplete read buffer", status),
                    glow::FRAMEBUFFER_UNSUPPORTED => format!("({status}) Framebuffer unsupported"),
                    _ => format!("({status}) Framebuffer not complete!"),
                };

                return Err(ErrorKind::RenderTargetError(reason));
            }

            context.bind_framebuffer(glow::FRAMEBUFFER, None);
        }

        Ok(Self {
            context: context.clone(),
            fbo,
            stencil_rbo: Some(stencil_rbo),
        })
    }

    pub fn bind(&self) {
        unsafe {
            self.context.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
        }
    }

    pub fn unbind(context: &Rc<glow::Context>) {
        unsafe {
            context.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    // pub fn blit_to_texture(&self, texture: &GlTexture) {
    //     let dest_fbo = Self::new(texture);

    //     unsafe {
    //         glow::BindFramebuffer(glow::READ_FRAMEBUFFER, self.fbo);
    //         glow::BindFramebuffer(glow::DRAW_FRAMEBUFFER, dest_fbo.fbo);

    //         glow::BlitFramebuffer(
    //             0,
    //             0,
    //             self.width as i32,
    //             self.height as i32,
    //             0,
    //             0,
    //             dest_fbo.width as i32,
    //             dest_fbo.height as i32,
    //             glow::COLOR_BUFFER_BIT,
    //             glow::NEAREST
    //         );

    //         glow::BindFramebuffer(glow::READ_FRAMEBUFFER, 0);
    //         glow::BindFramebuffer(glow::DRAW_FRAMEBUFFER, 0);
    //     }
    // }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            self.context.delete_framebuffer(self.fbo);
            if let Some(stencil_rbo) = self.stencil_rbo {
                self.context.delete_renderbuffer(stencil_rbo);
            }
        }
    }
}
