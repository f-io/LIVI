use std::collections::BTreeMap;

use crate::{
    image::ImageStore,
    paint::{GradientStop, MultiStopGradient},
    Color, ErrorKind, ImageFlags, ImageId, ImageInfo, ImageSource, Renderer,
};

/// `GradientStore` holds image ids for multi-stop gradients. The actual image/textures
/// are contained by the Canvas's `ImageStore`.
//
// If many gradients are used in a frame, we could combine them into a single texture
// and update the texture immediately prior to giving the renderer the command list.
#[derive(Debug)]
pub struct GradientStore {
    this_frame: BTreeMap<MultiStopGradient, ImageId>,
    prev_frame: BTreeMap<MultiStopGradient, ImageId>,
}

impl GradientStore {
    /// Create a new empty gradient store
    pub fn new() -> Self {
        Self {
            this_frame: BTreeMap::new(),
            prev_frame: BTreeMap::new(),
        }
    }

    /// Lookup or add a multi-stop gradient in this gradient store.
    pub fn lookup_or_add<R: Renderer>(
        &mut self,
        colors: &MultiStopGradient,
        images: &mut ImageStore<R::Image>,
        renderer: &mut R,
    ) -> Result<ImageId, ErrorKind> {
        if let Some(gradient_image_id) = self.prev_frame.remove(colors) {
            // See if we already have this texture from the previous frame. If we find
            // it then we migrate it to the current frame so we don't release it and
            // return the texture id to the caller.
            self.this_frame.insert(colors.clone(), gradient_image_id);
            Ok(gradient_image_id)
        } else if let Some(gradient_image_id) = self.this_frame.get(colors) {
            // See if we already used this gradient in this frame, and return the texture
            // id if we do.
            Ok(*gradient_image_id)
        } else {
            // We need to allocate a texture and synthesize the gradient image.
            let info = ImageInfo::new(ImageFlags::REPEAT_Y, 256, 1, crate::PixelFormat::Rgba8);
            let gradient_image_id = images.alloc(renderer, info)?;
            let image = linear_gradient_stops(colors);
            images.update(renderer, gradient_image_id, ImageSource::Rgba(image.as_ref()), 0, 0)?;

            self.this_frame.insert(colors.clone(), gradient_image_id);
            Ok(gradient_image_id)
        }
    }

    /// Release the textures that were not used in the most recently rendered frame. This
    /// method should be called when all the commands have been submitted.
    pub fn release_old_gradients<R: Renderer>(&mut self, images: &mut ImageStore<R::Image>, renderer: &mut R) {
        let mut prev_textures = BTreeMap::new();
        std::mem::swap(&mut prev_textures, &mut self.prev_frame);
        for (_, gradient_image_id) in prev_textures {
            images.remove(renderer, gradient_image_id);
        }
        // Move the "this_frame" textures to "prev_frame". "prev_frame" is already empty.
        std::mem::swap(&mut self.this_frame, &mut self.prev_frame);
    }
}

#[allow(clippy::many_single_char_names)]
// Gradient filling, adapted from https://github.com/lieff/lvg/blob/master/render/common.c#L147
fn gradient_span(dest: &mut [rgb::RGBA8; 256], color0: Color, color1: Color, offset0: f32, offset1: f32) {
    let s0o = offset0.clamp(0.0, 1.0);
    let s1o = offset1.clamp(0.0, 1.0);

    if s1o < s0o {
        return;
    }

    let s = (s0o * 256.0) as usize;
    let e = (s1o * 256.0) as usize;

    let mut r = color0.r;
    let mut g = color0.g;
    let mut b = color0.b;
    let mut a = color0.a;

    let steps = (e - s) as f32;

    let dr = (color1.r - r) / steps;
    let dg = (color1.g - g) / steps;
    let db = (color1.b - b) / steps;
    let da = (color1.a - a) / steps;

    #[allow(clippy::needless_range_loop)]
    for i in s..e {
        // The output must be premultiplied, but we don't premultiply until this point
        // so that we can do gradients from transparent colors correctly -- for example
        // if we have a stop that is fully transparent red and it transitions to opaque
        // blue, we should see some red in the gradient. If we premultiply the stops
        // then we won't see any red, because we will have already multiplied it to zero.
        // This way we'll get the red contribution.
        dest[i] = rgb::RGBA8::new(
            (r * a * 255.0) as u8,
            (g * a * 255.0) as u8,
            (b * a * 255.0) as u8,
            (a * 255.0) as u8,
        );
        r += dr;
        g += dg;
        b += db;
        a += da;
    }
}
fn linear_gradient_stops(gradient: &MultiStopGradient) -> imgref::Img<Vec<rgb::RGBA8>> {
    let mut dest = [rgb::RGBA8::new(0, 0, 0, 0); 256];

    // Fill the gradient up to the first stop.
    let first_stop = gradient.get(0);
    if first_stop.0 > 0.0 {
        let s0 = first_stop.0;
        let color0 = first_stop.1;
        gradient_span(&mut dest, color0, color0, 0.0, s0);
    }

    // Iterate over the stops in overlapping pairs and fill out the rest of the
    // gradient. If the stop position is > 1.0 then we have exhausted the stops
    // and should break. As a special case, if the second stop is > 1.0 then we
    // fill the current color to the end of the gradient.
    for [GradientStop(s0, color0), GradientStop(s1, color1)] in gradient.pairs() {
        // Catch the case where the last stop doesn't go all the way to 1.0 and
        // pad it.
        if s0 < 1.0 && s1 > 1.0 {
            gradient_span(&mut dest, color0, color0, s0, 1.0);
        } else {
            gradient_span(&mut dest, color0, color1, s0, s1);
        }

        // If the first stop is >1.0 then we're done.
        if s0 > 1.0 {
            break;
        }
    }
    imgref::Img::new(dest.to_vec(), 256, 1)
}
