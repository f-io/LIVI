// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

pub fn rgb(hex: &str) -> Option<[u8; 3]> {
    crate::shell::hex_color(hex)
}

pub fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn to_hsl([r, g, b]: [u8; 3]) -> Hsl {
    let (r, g, b) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return Hsl {
            h: 0.0,
            s: 0.0,
            l: (l * 100.0).round(),
        };
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    Hsl {
        h: h.round() % 360.0,
        s: (s * 100.0).round(),
        l: (l * 100.0).round(),
    }
}

pub fn from_hsl(c: Hsl) -> [u8; 3] {
    let s = c.s / 100.0;
    let l = c.l / 100.0;
    let k = |n: f64| (n + c.h / 30.0) % 12.0;
    let a = s * l.min(1.0 - l);
    let f = |n: f64| {
        let v = l - a * (-1.0f64).max((k(n) - 3.0).min((9.0 - k(n)).min(1.0)));
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [f(0.0), f(8.0), f(4.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for c in [
            [0u8, 0xad, 0xad],
            [0, 0, 0],
            [255, 255, 255],
            [0x12, 0x34, 0x56],
        ] {
            let back = from_hsl(to_hsl(c));
            for i in 0..3 {
                assert!(
                    (i16::from(back[i]) - i16::from(c[i])).abs() <= 2,
                    "{c:?} -> {back:?}"
                );
            }
        }
        assert_eq!(hex([0, 0xad, 0xad]), "#00adad");
    }

    #[test]
    fn whole_degree_sliders_cannot_hold_every_hex() {
        let c = [0xff, 0x7e, 0x00];
        assert_eq!(to_hsl(c).h, 30.0);
        assert_eq!(from_hsl(to_hsl(c)), [0xff, 0x80, 0x00]);
    }
}
