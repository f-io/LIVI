/// Struct representing a color with red, green, blue, and alpha components.
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
        Self { r, g, b, a: 1.0 }
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
        let mut h = h % 1.0;

        if h < 0.0 {
            h += 1.0;
        }

        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);

        let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let m1 = 2.0 * l - m2;

        Self {
            r: hue(h + 1.0 / 3.0, m1, m2).clamp(0.0, 1.0),
            g: hue(h, m1, m2).clamp(0.0, 1.0),
            b: hue(h - 1.0 / 3.0, m1, m2).clamp(0.0, 1.0),
            a,
        }
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
