// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn round_half_up(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_towards_positive_infinity() {
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(-2.5), -2.0);
        assert_eq!(round_half_up(-2.6), -3.0);
        assert_eq!(round_half_up(0.499_999_999_999_999_94), 0.0);
    }
}
