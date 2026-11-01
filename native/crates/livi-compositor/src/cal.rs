// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_host_proto::display::Correction;

use crate::state::CalState;

pub fn curves(cal: &CalState, size: usize) -> [Vec<u16>; 3] {
    let correction = Correction {
        gamma: f64::from(cal.gamma),
        contrast: f64::from(cal.contrast),
        gain: cal.gain.map(f64::from),
    };
    correction.curves(size).map(|c| {
        c.iter()
            .map(|v| (v * f64::from(u16::MAX)).round() as u16)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_spans_the_whole_16_bit_range() {
        let cal = CalState {
            in_lut: false,
            active: true,
            gamma: 1.0,
            contrast: 1.0,
            gain: [0.5, 1.0, 1.0],
        };
        let [r, g, _] = curves(&cal, 256);
        assert_eq!((r[0], r[255], g[255]), (0, 32768, u16::MAX));
    }
}
