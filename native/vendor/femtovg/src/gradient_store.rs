use std::collections::BTreeMap;

use crate::{
    color::{from_space_components, resolve_hue_arc, to_space_components},
    image::ImageStore,
    paint::{GradientStop, MultiStopGradient},
    Color, ColorSpace, ErrorKind, ImageFlags, ImageId, ImageInfo, ImageSource, Renderer,
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
fn gradient_span(
    dest: &mut [rgb::RGBA8; 256],
    color0: Color,
    color1: Color,
    offset0: f32,
    offset1: f32,
    space: ColorSpace,
    premultiplied: bool,
) {
    let s0o = offset0.clamp(0.0, 1.0);
    let s1o = offset1.clamp(0.0, 1.0);

    // Empty, reversed and NaN spans all take this exit.
    if s1o.partial_cmp(&s0o) != Some(std::cmp::Ordering::Greater) {
        return;
    }

    // Texel `i` holds the color at `i / 255`, so the shaders' `d = 0` and
    // `d = 1` land on the centres of the end texels and the pads come out
    // exact. A span takes the texels in [offset0, offset1); the last one
    // also takes texel 255.
    let s = (s0o * 255.0).ceil() as usize;
    let e = if s1o >= 1.0 { 256 } else { (s1o * 255.0).ceil() as usize };

    let (c0, c1) = resolve_hue_arc(
        to_space_components(color0, space),
        to_space_components(color1, space),
        space,
    );

    // CSS Color 4 premultiplies every component but the hue, in the interpolation space.
    let hue = space.hue_layout().map(|(hue, _)| hue);
    let scale = |mut c: [f32; 3], factor: f32| {
        for (i, component) in c.iter_mut().enumerate() {
            if Some(i) != hue {
                *component *= factor;
            }
        }
        c
    };
    let (c0, c1) = if premultiplied {
        (scale(c0, color0.a), scale(c1, color1.a))
    } else {
        (c0, c1)
    };

    let len = s1o - s0o;
    #[allow(clippy::needless_range_loop)]
    for i in s..e {
        let t = ((i as f32 / 255.0 - s0o) / len).clamp(0.0, 1.0);
        let c = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * t);
        let a = color0.a + (color1.a - color0.a) * t;

        // The output must be premultiplied. Straight interpolation doesn't premultiply until
        // this point, so that a gradient from a fully transparent red stop to opaque blue
        // shows some red, as Canvas 2D and SVG require. Premultiplied interpolation (CSS)
        // has already multiplied the red to zero, so the same gradient only fades in blue.
        let straight = if premultiplied && a > 0.0 { scale(c, 1.0 / a) } else { c };
        let [cr, cg, cb] = from_space_components(straight, space);
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        dest[i] = rgb::RGBA8::new(to_u8(cr * a), to_u8(cg * a), to_u8(cb * a), to_u8(a));
    }
}
fn linear_gradient_stops(gradient: &MultiStopGradient) -> imgref::Img<Vec<rgb::RGBA8>> {
    let space = gradient.space();
    let premultiplied = gradient.premultiplied();
    let mut dest = [rgb::RGBA8::new(0, 0, 0, 0); 256];

    // Fill the gradient up to the first stop.
    let first_stop = gradient.get(0);
    if first_stop.0 > 0.0 {
        let s0 = first_stop.0;
        let color0 = first_stop.1;
        gradient_span(&mut dest, color0, color0, 0.0, s0, space, premultiplied);
    }

    // Iterate over the stops in overlapping pairs and fill out the rest of the
    // gradient. If the stop position is > 1.0 then we have exhausted the stops
    // and should break. As a special case, if the second stop is > 1.0 then we
    // fill the current color to the end of the gradient.
    for [GradientStop(s0, color0), GradientStop(s1, color1)] in gradient.pairs() {
        // Catch the case where the last stop doesn't go all the way to 1.0 and
        // pad it.
        if s0 < 1.0 && s1 > 1.0 {
            gradient_span(&mut dest, color0, color0, s0, 1.0, space, premultiplied);
        } else {
            gradient_span(&mut dest, color0, color1, s0, s1, space, premultiplied);
        }

        // If the first stop is >1.0 then we're done.
        if s0 > 1.0 {
            break;
        }
    }

    // Pad from the last stop to the end of the ramp, mirroring the head pad
    // above: SVG `spreadMethod="pad"` and Canvas gradients clamp to the last
    // stop's color. Without this the texels past a last stop below 1.0 were
    // never written - transparent on a fresh texture, stale on a recycled one.
    let last_stop = gradient.get(gradient.len() - 1);
    if last_stop.0 < 1.0 {
        gradient_span(
            &mut dest,
            last_stop.1,
            last_stop.1,
            last_stop.0,
            1.0,
            space,
            premultiplied,
        );
    }
    imgref::Img::new(dest.to_vec(), 256, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::GradientColors;

    fn lut(stops: Vec<(f32, Color)>) -> Vec<rgb::RGBA8> {
        lut_in_space(stops, ColorSpace::Srgb)
    }

    fn lut_in_space(stops: Vec<(f32, Color)>, space: ColorSpace) -> Vec<rgb::RGBA8> {
        lut_with(stops, space, false)
    }

    fn premultiplied_lut(stops: Vec<(f32, Color)>, space: ColorSpace) -> Vec<rgb::RGBA8> {
        lut_with(stops, space, true)
    }

    fn lut_with(stops: Vec<(f32, Color)>, space: ColorSpace, premultiplied: bool) -> Vec<rgb::RGBA8> {
        let mut colors = GradientColors::from_stops(stops);
        colors.set_interpolation_space(space);
        colors.set_premultiplied_interpolation(premultiplied);
        match colors {
            GradientColors::MultiStop { stops } => linear_gradient_stops(&stops).buf().to_vec(),
            GradientColors::TwoStop { .. } => panic!("expected a multi-stop gradient"),
        }
    }

    /// CSS interpolates premultiplied: a transparent black stop adds no black, so white fades
    /// out as white instead of passing through gray as straight interpolation does.
    #[test]
    fn premultiplied_interpolation_fades_without_the_transparent_stops_color() {
        let stops = vec![
            (0.0, Color::white()),
            (0.5, Color::rgba(0, 0, 0, 0)),
            (1.0, Color::rgba(0, 0, 0, 0)),
        ];
        let middle = 64;

        let straight = lut(stops.clone())[middle];
        assert_eq!(straight.a, 127);
        assert_rgb_close(straight, (63, 63, 63), 1);

        let premultiplied = premultiplied_lut(stops, ColorSpace::Srgb)[middle];
        assert_eq!(premultiplied.a, 127);
        assert_rgb_close(premultiplied, (127, 127, 127), 1);
    }

    /// Between stops of equal alpha the premultiplied and straight ramps are the same.
    #[test]
    fn premultiplied_interpolation_matches_straight_for_equal_alphas() {
        let stops = vec![
            (0.0, Color::rgba(255, 0, 0, 128)),
            (0.5, Color::rgba(0, 0, 255, 128)),
            (1.0, Color::rgba(0, 255, 0, 128)),
        ];
        for space in [ColorSpace::Srgb, ColorSpace::Oklab, ColorSpace::Oklch] {
            let straight = lut_in_space(stops.clone(), space);
            let premultiplied = premultiplied_lut(stops.clone(), space);
            for (s, p) in straight.iter().zip(&premultiplied) {
                assert_rgb_close(*p, (s.r, s.g, s.b), 1);
                assert_eq!(p.a, s.a);
            }
        }
    }

    fn assert_rgb_close(got: rgb::RGBA8, want: (u8, u8, u8), tol: i16) {
        let close = |g: u8, w: u8| (g as i16 - w as i16).abs() <= tol;
        assert!(
            close(got.r, want.0) && close(got.g, want.1) && close(got.b, want.2),
            "{got:?} not within {tol} of {want:?}"
        );
    }

    /// An Oklab red-to-green ramp keeps its endpoints but avoids the muddy sRGB midpoint.
    #[test]
    fn oklab_interpolation_round_trips_endpoints_and_differs_from_srgb() {
        // The duplicate first stop forces the `MultiStop` path.
        let stops = || {
            vec![
                (0.0, Color::rgb(255, 0, 0)),
                (0.0, Color::rgb(255, 0, 0)),
                (1.0, Color::rgb(0, 255, 0)),
            ]
        };

        let srgb = lut_in_space(stops(), ColorSpace::Srgb);
        let oklab = lut_in_space(stops(), ColorSpace::Oklab);

        for texels in [&srgb, &oklab] {
            // The end texels hold the stops, up to the Oklab round trip.
            assert_rgb_close(texels[0], (255, 0, 0), 1);
            assert_rgb_close(texels[255], (0, 255, 0), 1);
        }

        // Independently computed Oklab midpoint; sRGB gives (127, 127, 0).
        assert_rgb_close(oklab[128], (208, 168, 0), 3);
        assert_ne!(
            srgb[128], oklab[128],
            "sRGB and Oklab must interpolate the midpoint differently"
        );
    }

    /// SVG `spreadMethod="pad"` / Canvas semantics: past the last stop the
    /// ramp holds the last stop's color. Regression for the Firefox logo
    /// (splash-logo.svg, mr-settodefault.svg), whose 4-stop flame gradient
    /// ends at offset 0.70 and rendered its last 30% unwritten.
    #[test]
    fn ramp_pads_past_the_last_stop() {
        let last = Color::rgb(227, 21, 135);
        let texels = lut(vec![
            (0.05, Color::rgb(255, 244, 79)),
            (0.37, Color::rgb(255, 152, 14)),
            (0.53, Color::rgb(255, 54, 71)),
            (0.70, last),
        ]);
        for i in [180usize, 200, 230, 255] {
            let t = texels[i];
            assert_eq!(
                (t.r, t.g, t.b, t.a),
                (227, 21, 135, 255),
                "texel {i} must hold the last stop's color"
            );
        }
        // The head pad keeps working too.
        let head = texels[0];
        assert_eq!((head.r, head.g, head.b, head.a), (255, 244, 79, 255));
    }

    /// Black to white in linear light is brighter at the midpoint than the sRGB lerp.
    #[test]
    fn linear_rgb_midpoint_is_the_linear_light_average() {
        // The duplicate first stop forces the `MultiStop` path.
        let texels = lut_in_space(
            vec![(0.0, Color::black()), (0.0, Color::black()), (1.0, Color::white())],
            ColorSpace::LinearRgb,
        );
        // Linear 0.5 encodes to sRGB 0.735; sRGB interpolation gives 128.
        assert_rgb_close(texels[128], (187, 187, 187), 2);
    }

    /// Texel `i` holds the color at `i / 255`, where the shaders sample it.
    /// LinearRGB rises steeply from zero, so any offset shows up near black.
    #[test]
    fn ramp_texels_hold_the_color_where_they_are_sampled() {
        // The duplicate first stop forces the `MultiStop` path.
        let texels = lut_in_space(
            vec![(0.0, Color::black()), (0.0, Color::black()), (1.0, Color::white())],
            ColorSpace::LinearRgb,
        );
        for (i, texel) in texels.iter().enumerate() {
            let want = from_space_components([i as f32 / 255.0; 3], ColorSpace::LinearRgb)[0] * 255.0;
            assert!(
                (texel.r as f32 - want).abs() <= 0.5 + 1e-3,
                "texel {i}: {} vs {want}",
                texel.r
            );
        }
    }

    /// Red to cyan is exactly half a turn; the tie must keep increasing hue.
    #[test]
    fn hsl_antipodal_gradient_goes_forward_through_green_not_backward_through_blue() {
        // The duplicate first stop forces the `MultiStop` path.
        let texels = lut_in_space(
            vec![
                (0.0, Color::rgb(255, 0, 0)),
                (0.0, Color::rgb(255, 0, 0)),
                (1.0, Color::rgb(0, 255, 255)),
            ],
            ColorSpace::Hsl,
        );
        // h=0.25, s=1, l=0.5 is chartreuse.
        assert_rgb_close(texels[128], (128, 255, 0), 3);
    }

    /// `GradientStore` caches textures by `Ord`, so `space` must be part of it.
    #[test]
    fn multi_stop_gradients_differing_only_in_space_are_not_equal() {
        let stops = || {
            vec![
                (0.0, Color::rgb(255, 0, 0)),
                (0.0, Color::rgb(255, 0, 0)),
                (1.0, Color::rgb(0, 255, 0)),
            ]
        };
        let srgb = match GradientColors::from_stops(stops()) {
            GradientColors::MultiStop { stops } => stops,
            GradientColors::TwoStop { .. } => panic!("expected a multi-stop gradient"),
        };
        let mut oklab = srgb.clone();
        oklab.set_interpolation_space(ColorSpace::Oklab);

        assert_ne!(srgb.cmp(&oklab), std::cmp::Ordering::Equal);
    }

    /// A transparent last stop pads transparent (premultiplied zero), not
    /// black.
    #[test]
    fn transparent_last_stop_pads_transparent() {
        let texels = lut(vec![
            (0.0, Color::rgb(255, 0, 0)),
            (0.4, Color::rgb(0, 255, 0)),
            (0.6, Color::rgba(0, 0, 255, 0)),
        ]);
        let t = texels[255];
        assert_eq!((t.r, t.g, t.b, t.a), (0, 0, 0, 0));
    }

    /// Empty, reversed and NaN spans write nothing.
    #[test]
    fn degenerate_spans_leave_the_ramp_untouched() {
        let untouched = rgb::RGBA8::new(1, 2, 3, 4);
        let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        for (offset0, offset1) in [
            (0.5, 0.5),
            (0.6, 0.4),
            (f32::NAN, 0.5),
            (0.5, f32::NAN),
            (f32::NAN, f32::NAN),
        ] {
            let mut dest = [untouched; 256];
            gradient_span(&mut dest, red, blue, offset0, offset1, ColorSpace::Srgb, false);
            assert!(
                dest.iter().all(|&t| t == untouched),
                "span {offset0}..{offset1} wrote texels"
            );
        }
    }

    #[test]
    fn three_stop_ramp_is_unchanged() {
        let texels = lut(vec![
            (0.0, Color::rgb(255, 0, 0)),
            (0.5, Color::rgb(0, 255, 0)),
            (1.0, Color::rgb(0, 0, 255)),
        ]);
        let rgba = |i: usize| {
            let t = texels[i];
            (t.r, t.g, t.b, t.a)
        };
        // Texel `i` holds the color at `i / 255`, so the ends are the stops.
        assert_eq!(rgba(0), (255, 0, 0, 255));
        assert_eq!(rgba(64), (127, 128, 0, 255));
        assert_eq!(rgba(128), (0, 254, 1, 255));
        assert_eq!(rgba(192), (0, 126, 129, 255));
        assert_eq!(rgba(255), (0, 0, 255, 255));
    }

    /// Coincident stops make a hard edge: each side keeps its own span.
    #[test]
    fn coincident_stops_make_a_hard_edge() {
        let texels = lut(vec![
            (0.0, Color::rgb(255, 0, 0)),
            (0.5, Color::rgb(255, 0, 0)),
            (0.5, Color::rgb(0, 0, 255)),
            (1.0, Color::rgb(0, 0, 255)),
        ]);
        assert_eq!((texels[127].r, texels[127].b), (255, 0));
        assert_eq!((texels[128].r, texels[128].b), (0, 255));
    }
}
