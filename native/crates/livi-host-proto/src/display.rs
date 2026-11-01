// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correction {
    pub gamma: f64,
    pub contrast: f64,
    pub gain: [f64; 3],
}

impl Default for Correction {
    fn default() -> Self {
        Self {
            gamma: 1.0,
            contrast: 1.0,
            gain: [1.0; 3],
        }
    }
}

impl Correction {
    pub fn from_line(v: [f64; 5]) -> Self {
        Self {
            gamma: v[0],
            contrast: v[1],
            gain: [v[2], v[3], v[4]],
        }
    }

    pub fn neutral(&self) -> bool {
        *self == Self::default()
    }

    pub fn curve(&self, channel: usize, size: usize) -> Vec<f64> {
        let last = size.saturating_sub(1).max(1) as f64;
        let gain = self.gain[channel.min(2)];
        (0..size)
            .map(|i| {
                let c = (i as f64 / last).powf(1.0 / self.gamma.max(0.01));
                let c = (c - 0.5) * self.contrast + 0.5;
                (c * gain).clamp(0.0, 1.0)
            })
            .collect()
    }

    pub fn curves(&self, size: usize) -> [Vec<f64>; 3] {
        [0, 1, 2].map(|c| self.curve(c, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_values_give_a_straight_ramp() {
        let c = Correction::default();
        assert!(c.neutral());
        let ramp = c.curve(0, 256);
        assert_eq!((ramp[0], ramp[255]), (0.0, 1.0));
        assert!((ramp[128] - 128.0 / 255.0).abs() < 1e-12);
    }

    #[test]
    fn gain_contrast_and_gamma_bend_the_ramp() {
        let half_red = Correction::from_line([1.0, 1.0, 0.5, 1.0, 1.0]);
        let [r, g, _] = half_red.curves(256);
        assert_eq!((r[255], g[255]), (0.5, 1.0));
        let flat = Correction::from_line([1.0, 0.5, 1.0, 1.0, 1.0]);
        assert_eq!(flat.curve(1, 3), vec![0.25, 0.5, 0.75]);
        let bright = Correction::from_line([1.5, 1.0, 1.0, 1.0, 1.0]);
        assert!(bright.curve(2, 256)[64] > Correction::default().curve(2, 256)[64]);
    }
}
