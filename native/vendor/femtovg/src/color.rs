/// Color space that gradient stops are interpolated in.
/// Set with [`Paint::with_interpolation_space`](crate::Paint::with_interpolation_space).
///
/// Differences from CSS Color 4:
/// - Alpha is interpolated straight unless
///   [`Paint::with_premultiplied_interpolation`](crate::Paint::with_premultiplied_interpolation) asks for premultiplied.
/// - Out-of-gamut `Oklab`/`Oklch` results are clamped per channel, not gamut-mapped,
///   so very saturated ramps can shift hue slightly.
/// - In `Oklch`/`Hsl`, an achromatic stop (grey, white, black) takes the other stop's hue.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ColorSpace {
    /// Interpolate directly in sRGB (the default).
    #[default]
    Srgb,
    /// Interpolate in linear-light sRGB (SVG `linearRGB`, CSS `srgb-linear`).
    LinearRgb,
    /// Interpolate in Oklab.
    Oklab,
    /// Interpolate in Oklch, taking the shorter hue arc.
    Oklch,
    /// Interpolate in HSL, taking the shorter hue arc.
    Hsl,
}

impl ColorSpace {
    /// Indices of hue and chroma/saturation in the component triple,
    /// or `None` for spaces without hue.
    pub(crate) fn hue_layout(self) -> Option<(usize, usize)> {
        match self {
            Self::Oklch => Some((2, 1)),
            Self::Hsl => Some((0, 1)),
            Self::Srgb | Self::LinearRgb | Self::Oklab => None,
        }
    }
}

/// Struct representing a color with red, green, blue, and alpha components.
///
/// Always sRGB, regardless of a gradient's [`ColorSpace`].
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Color {
    /// Red component of the color (0.0 to 1.0)
    pub r: f32,
    /// Green component of the color (0.0 to 1.0)
    pub g: f32,
    /// Blue component of the color (0.0 to 1.0)
    pub b: f32,
    /// Alpha (opacity) component of the color (0.0 to 1.0)
    pub a: f32,
}

impl Color {
    /// Creates a color from red, green, and blue u8 values. Alpha is set to 255.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgbf(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    /// Creates a color from red, green, and blue f32 values. Alpha is set to 1.0.
    pub const fn rgbf(r: f32, g: f32, b: f32) -> Self {
        Self::rgbaf(r, g, b, 1.0)
    }

    /// Creates a color from red, green, blue, and alpha u8 values.
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgbaf(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0)
    }

    /// Creates a color from red, green, blue, and alpha f32 values.
    pub const fn rgbaf(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a color from hue, saturation, and lightness f32 values. Alpha is set to 1.0.
    /// All values are all in range [0..1].
    pub fn hsl(h: f32, s: f32, l: f32) -> Self {
        Self::hsla(h, s, l, 1.0)
    }

    /// Creates a color from hue, saturation, lightness, and alpha f32 values.
    /// All values are all in range [0..1].
    pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> Self {
        let (r, g, b) = hsl_to_srgb(h, s, l);
        Self { r, g, b, a }
    }

    /// Creates a color from a 6-digit (`RRGGBB`) or 8-digit (`RRGGBBAA`) HTML hexadecimal string.
    /// Any other length produces `rgb(0,0,0)`.
    /// The “#” is optional.
    pub fn hex(raw_hex: &str) -> Self {
        let hex = raw_hex.trim_start_matches('#');

        if hex.len() == 8 {
            Self::rgba(
                hex_to_u8(&hex[0..2]),
                hex_to_u8(&hex[2..4]),
                hex_to_u8(&hex[4..6]),
                hex_to_u8(&hex[6..8]),
            )
        } else if hex.len() == 6 {
            Self::rgb(hex_to_u8(&hex[0..2]), hex_to_u8(&hex[2..4]), hex_to_u8(&hex[4..6]))
        } else {
            Self::rgb(0, 0, 0)
        }
    }

    /// Returns a white color (1.0, 1.0, 1.0, 1.0)
    pub const fn white() -> Self {
        Self::rgbaf(1.0, 1.0, 1.0, 1.0)
    }

    /// Returns a black color (0.0, 0.0, 0.0, 1.0)
    pub const fn black() -> Self {
        Self::rgbaf(0.0, 0.0, 0.0, 1.0)
    }

    /// Sets the alpha (opacity) component of the color from a u8 value.
    pub fn set_alpha(&mut self, a: u8) {
        self.set_alphaf(a as f32 / 255.0);
    }

    /// Sets the alpha (opacity) component of the color from an f32 value.
    pub fn set_alphaf(&mut self, a: f32) {
        self.a = a;
    }

    /// Returns a color with premultiplied alpha components.
    pub fn premultiplied(self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
        }
    }

    /// Converts the color to a [f32; 4] array.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Checks if the color is black (0.0, 0.0, 0.0, 0.0)
    pub fn is_black(&self) -> bool {
        self.r == 0.0 && self.g == 0.0 && self.b == 0.0 && self.a == 0.0
    }
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

/// Converts `color`'s sRGB components into `space`'s component triple.
pub(crate) fn to_space_components(color: Color, space: ColorSpace) -> [f32; 3] {
    match space {
        ColorSpace::Srgb => [color.r, color.g, color.b],
        ColorSpace::LinearRgb => [color.r, color.g, color.b].map(srgb_channel_to_linear),
        ColorSpace::Oklab => {
            let (l, a, b) = srgb_to_oklab(color.r, color.g, color.b);
            [l, a, b]
        }
        ColorSpace::Oklch => {
            let (l, c, h) = srgb_to_oklch(color.r, color.g, color.b);
            [l, c, h]
        }
        ColorSpace::Hsl => {
            let (h, s, l) = srgb_to_hsl(color.r, color.g, color.b);
            [h, s, l]
        }
    }
}

/// Inverse of [`to_space_components`].
pub(crate) fn from_space_components(c: [f32; 3], space: ColorSpace) -> [f32; 3] {
    match space {
        ColorSpace::Srgb => c,
        ColorSpace::LinearRgb => c.map(linear_channel_to_srgb),
        ColorSpace::Oklab => {
            let (r, g, b) = oklab_to_srgb(c[0], c[1], c[2]);
            [r, g, b]
        }
        ColorSpace::Oklch => {
            let (r, g, b) = oklch_to_srgb(c[0], c[1], c[2]);
            [r, g, b]
        }
        ColorSpace::Hsl => {
            let (r, g, b) = hsl_to_srgb(c[0], c[1], c[2]);
            [r, g, b]
        }
    }
}

/// Chroma/saturation at or below this counts as zero.
/// Float noise keeps achromatic colors slightly off zero.
const HUE_MAGNITUDE_POWERLESS_THRESHOLD: f32 = 1e-4;

/// An achromatic color's hue is arbitrary; CSS Color 4 calls it "powerless".
fn is_hue_powerless(c: [f32; 3], space: ColorSpace) -> bool {
    match space.hue_layout() {
        Some((_, magnitude_index)) => c[magnitude_index] <= HUE_MAGNITUDE_POWERLESS_THRESHOLD,
        None => false,
    }
}

/// Prepares hue-based endpoints for interpolation.
/// A powerless hue borrows the other endpoint's,
/// then `to`'s hue is wrapped onto the shorter arc.
/// A no-op for spaces without hue.
pub(crate) fn resolve_hue_arc(mut from: [f32; 3], mut to: [f32; 3], space: ColorSpace) -> ([f32; 3], [f32; 3]) {
    let Some((i, _)) = space.hue_layout() else {
        return (from, to);
    };

    let from_powerless = is_hue_powerless(from, space);
    let to_powerless = is_hue_powerless(to, space);
    if from_powerless && !to_powerless {
        from[i] = to[i];
    } else if to_powerless && !from_powerless {
        to[i] = from[i];
    }

    // Strict comparison, as in CSS Color 4 `shorter`.
    let diff = to[i] - from[i];
    if diff > 0.5 {
        to[i] -= 1.0;
    } else if diff < -0.5 {
        to[i] += 1.0;
    }
    (from, to)
}

fn srgb_channel_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_channel_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (r, g, b) = (
        srgb_channel_to_linear(r),
        srgb_channel_to_linear(g),
        srgb_channel_to_linear(b),
    );

    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());

    (
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    )
}

// Out-of-gamut results are clamped in linear space per channel before encoding
fn oklab_to_srgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);

    let r = (4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s).clamp(0.0, 1.0);
    let g = (-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s).clamp(0.0, 1.0);
    let b = (-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s).clamp(0.0, 1.0);

    (
        linear_channel_to_srgb(r),
        linear_channel_to_srgb(g),
        linear_channel_to_srgb(b),
    )
}

// `h` is in turns, like `Color::hsl`.
fn srgb_to_oklch(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (l, a, ob) = srgb_to_oklab(r, g, b);
    let c = a.hypot(ob);
    let h = ob.atan2(a) / std::f32::consts::TAU;
    (l, c, if h < 0.0 { h + 1.0 } else { h })
}

fn oklch_to_srgb(l: f32, c: f32, h: f32) -> (f32, f32, f32) {
    let angle = h * std::f32::consts::TAU;
    oklab_to_srgb(l, c * angle.cos(), c * angle.sin())
}

// `h` is in turns.
fn srgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if max == min {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let mut h = if max == r {
        ((g - b) / d) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    if h < 0.0 {
        h += 1.0;
    }
    (h, s, l)
}

fn hsl_to_srgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let mut h = h % 1.0;
    if h < 0.0 {
        h += 1.0;
    }

    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);

    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;

    (
        hue(h + 1.0 / 3.0, m1, m2).clamp(0.0, 1.0),
        hue(h, m1, m2).clamp(0.0, 1.0),
        hue(h - 1.0 / 3.0, m1, m2).clamp(0.0, 1.0),
    )
}

fn hue(mut h: f32, m1: f32, m2: f32) -> f32 {
    if h < 0.0 {
        h += 1.0;
    }
    if h > 1.0 {
        h -= 1.0;
    }

    if h < 1.0 / 6.0 {
        return m1 + (m2 - m1) * h * 6.0;
    }
    if h < 3.0 / 6.0 {
        return m2;
    }
    if h < 4.0 / 6.0 {
        return m1 + (m2 - m1) * (2.0 / 3.0 - h) * 6.0;
    }

    m1
}

// Convert a hex string to decimal. Eg. "00" -> 0. "FF" -> 255.
fn hex_to_u8(hex_string: &str) -> u8 {
    u8::from_str_radix(hex_string, 16).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f32, b: f32, tol: f32) {
        assert!((a - b).abs() <= tol, "{a} not within {tol} of {b}");
    }

    // Hue wraps at 1.0, so compare distance around the circle.
    fn assert_hues_close(a: f32, b: f32, tol: f32) {
        let diff = (a - b).rem_euclid(1.0);
        let circular_diff = diff.min(1.0 - diff);
        assert!(circular_diff <= tol, "{a} turns not within {tol} of {b} turns");
    }

    #[test]
    fn to_space_components_srgb_is_identity() {
        let c = Color::rgb(200, 50, 10);
        assert_eq!(to_space_components(c, ColorSpace::Srgb), [c.r, c.g, c.b]);
    }

    #[test]
    fn white_and_black_are_the_expected_oklab_landmarks() {
        let white = to_space_components(Color::white(), ColorSpace::Oklab);
        assert_close(white[0], 1.0, 1e-4);
        assert_close(white[1], 0.0, 1e-4);
        assert_close(white[2], 0.0, 1e-4);

        let black = to_space_components(Color::black(), ColorSpace::Oklab);
        assert_close(black[0], 0.0, 1e-4);
        assert_close(black[1], 0.0, 1e-4);
        assert_close(black[2], 0.0, 1e-4);
    }

    #[test]
    fn linear_rgb_round_trips_back_to_srgb() {
        for (r, g, b) in [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (30, 180, 220),
            (128, 128, 128),
            (0, 0, 0),
        ] {
            let original = Color::rgb(r, g, b);
            let linear = to_space_components(original, ColorSpace::LinearRgb);
            let [rr, gg, bb] = from_space_components(linear, ColorSpace::LinearRgb);
            assert_close(rr, original.r, 1e-3);
            assert_close(gg, original.g, 1e-3);
            assert_close(bb, original.b, 1e-3);
        }
    }

    #[test]
    fn oklab_round_trips_back_to_srgb() {
        for (r, g, b) in [(255, 0, 0), (0, 255, 0), (0, 0, 255), (30, 180, 220), (128, 128, 128)] {
            let original = Color::rgb(r, g, b);
            let oklab = to_space_components(original, ColorSpace::Oklab);
            let [rr, gg, bb] = from_space_components(oklab, ColorSpace::Oklab);
            assert_close(rr, original.r, 1e-3);
            assert_close(gg, original.g, 1e-3);
            assert_close(bb, original.b, 1e-3);
        }
    }

    #[test]
    fn oklch_round_trips_back_to_srgb() {
        for (r, g, b) in [(255, 0, 0), (0, 255, 0), (0, 0, 255), (30, 180, 220), (128, 128, 128)] {
            let original = Color::rgb(r, g, b);
            let oklch = to_space_components(original, ColorSpace::Oklch);
            let [rr, gg, bb] = from_space_components(oklch, ColorSpace::Oklch);
            assert_close(rr, original.r, 1e-3);
            assert_close(gg, original.g, 1e-3);
            assert_close(bb, original.b, 1e-3);
        }
    }

    #[test]
    fn hsl_round_trips_back_to_srgb() {
        for (r, g, b) in [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (30, 180, 220),
            (128, 128, 128),
            (0, 0, 0),
        ] {
            let original = Color::rgb(r, g, b);
            let hsl = to_space_components(original, ColorSpace::Hsl);
            let [rr, gg, bb] = from_space_components(hsl, ColorSpace::Hsl);
            assert_close(rr, original.r, 1e-3);
            assert_close(gg, original.g, 1e-3);
            assert_close(bb, original.b, 1e-3);
        }
    }

    #[test]
    fn srgb_to_hsl_recovers_the_hsl_constructors_inputs() {
        // A round trip through sRGB alone wouldn't catch a wrong but invertible `srgb_to_hsl`.
        for (h, s, l) in [(0.0, 0.8, 0.5), (0.25, 0.5, 0.3), (0.6, 1.0, 0.7), (0.9, 0.2, 0.9)] {
            let color = Color::hsl(h, s, l);
            let [hh, ss, ll] = to_space_components(color, ColorSpace::Hsl);
            assert_hues_close(hh, h, 1e-4);
            assert_close(ss, s, 1e-4);
            assert_close(ll, l, 1e-4);
        }
    }

    #[test]
    fn resolve_hue_arc_picks_the_short_way_round() {
        // 350deg -> 10deg must wrap through 0.
        let from = to_space_components(Color::hsl(350.0 / 360.0, 0.5, 0.5), ColorSpace::Hsl);
        let to = to_space_components(Color::hsl(10.0 / 360.0, 0.5, 0.5), ColorSpace::Hsl);
        let (_, shortened_to) = resolve_hue_arc(from, to, ColorSpace::Hsl);
        assert_close(shortened_to[0], 1.0 + 10.0 / 360.0, 1e-4);
    }

    #[test]
    fn resolve_hue_arc_leaves_an_exact_half_turn_tie_unwrapped() {
        // Red to cyan is exactly half a turn; the tie must keep increasing hue.
        let from = to_space_components(Color::rgb(255, 0, 0), ColorSpace::Hsl);
        let to = to_space_components(Color::rgb(0, 255, 255), ColorSpace::Hsl);
        assert_close(from[0], 0.0, 1e-4);
        assert_close(to[0], 0.5, 1e-4);
        let (_, shortened_to) = resolve_hue_arc(from, to, ColorSpace::Hsl);
        assert_close(shortened_to[0], 0.5, 1e-4);
    }

    #[test]
    fn resolve_hue_arc_is_a_no_op_for_non_hue_spaces() {
        let from = to_space_components(Color::rgb(255, 0, 0), ColorSpace::Oklab);
        let to = to_space_components(Color::rgb(0, 255, 255), ColorSpace::Oklab);
        assert_eq!(resolve_hue_arc(from, to, ColorSpace::Oklab), (from, to));
    }

    #[test]
    fn white_to_blue_holds_blues_hue_in_oklch() {
        // White's noisy hue is ~0.25 turns; the ramp must hold blue's hue instead.
        let white = to_space_components(Color::white(), ColorSpace::Oklch);
        let blue = to_space_components(Color::rgb(0, 0, 255), ColorSpace::Oklch);
        let (resolved_white, resolved_blue) = resolve_hue_arc(white, blue, ColorSpace::Oklch);
        assert_close(resolved_white[2], resolved_blue[2], 1e-6);
    }

    #[test]
    fn black_to_green_holds_greens_hue_in_hsl() {
        // Green, not red, since black's HSL hue is already 0.
        let black = to_space_components(Color::black(), ColorSpace::Hsl);
        let green = to_space_components(Color::rgb(0, 255, 0), ColorSpace::Hsl);
        let (resolved_black, resolved_green) = resolve_hue_arc(black, green, ColorSpace::Hsl);
        assert_close(resolved_black[0], resolved_green[0], 1e-6);
    }

    #[test]
    fn grey_to_grey_stays_a_no_op_when_both_endpoints_are_powerless() {
        let a = to_space_components(Color::rgb(64, 64, 64), ColorSpace::Hsl);
        let b = to_space_components(Color::rgb(200, 200, 200), ColorSpace::Hsl);
        let (resolved_a, resolved_b) = resolve_hue_arc(a, b, ColorSpace::Hsl);
        assert_close(resolved_a[0], a[0], 1e-6);
        assert_close(resolved_b[0], b[0], 1e-6);
    }
}
