// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::{Value, json};

use crate::settings::node::CustomPage;
use crate::settings::view::Change;

pub const LOW: f64 = 0.5;
pub const HIGH: f64 = 1.5;
const STEPS_PER_UNIT: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Colors,
    GrayWedge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Knob {
    pub key: &'static str,
    pub tint: Option<[u8; 3]>,
    pub icon: Option<&'static str>,
    pub value: f64,
}

impl Knob {
    pub fn level(&self) -> f32 {
        ((self.value - LOW) / (HIGH - LOW)).clamp(0.0, 1.0) as f32
    }

    pub fn text(&self) -> String {
        let v = format!("{:.2}", self.value);
        v.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    pub fn neutral(&self) -> bool {
        (self.value - 1.0).abs() < 1e-9
    }
}

fn knob(
    state: &Value,
    key: &'static str,
    tint: Option<[u8; 3]>,
    icon: Option<&'static str>,
) -> Knob {
    Knob {
        key,
        tint,
        icon,
        value: state["config"][key].as_f64().unwrap_or(1.0),
    }
}

pub fn page(page: CustomPage, state: &Value) -> Option<(Pattern, Vec<Knob>)> {
    match page {
        CustomPage::ColorCalibration => Some((
            Pattern::Colors,
            vec![
                knob(state, "displayColorR", Some([0xff, 0, 0]), None),
                knob(state, "displayColorG", Some([0, 0xff, 0]), None),
                knob(state, "displayColorB", Some([0, 0, 0xff]), None),
            ],
        )),
        CustomPage::ContrastGammaCalibration => Some((
            Pattern::GrayWedge,
            vec![
                knob(state, "displayGamma", None, Some("TonalityRounded")),
                knob(state, "displayContrast", None, Some("ContrastRounded")),
            ],
        )),
        _ => None,
    }
}

pub fn value_at(level: f32) -> f64 {
    let raw = LOW + f64::from(level.clamp(0.0, 1.0)) * (HIGH - LOW);
    (raw * STEPS_PER_UNIT).round() / STEPS_PER_UNIT
}

pub fn set(key: &str, value: f64) -> Change {
    Change::Config(json!({ key: value }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_color_page_has_three_tinted_knobs_from_the_config() {
        let state = json!({ "config": { "displayColorR": 1.2, "displayColorG": 1.0 } });
        let (pattern, knobs) = page(CustomPage::ColorCalibration, &state).unwrap();
        assert_eq!(pattern, Pattern::Colors);
        assert_eq!(knobs.len(), 3);
        assert_eq!(knobs[0].tint, Some([0xff, 0, 0]));
        assert_eq!(knobs[0].text(), "1.2");
        assert!((knobs[0].level() - 0.7).abs() < 1e-6);
        assert!(knobs[1].neutral());
        assert_eq!(knobs[2].value, 1.0);
        let (wedge, gamma) = page(CustomPage::ContrastGammaCalibration, &state).unwrap();
        assert_eq!(wedge, Pattern::GrayWedge);
        assert_eq!(gamma[0].icon, Some("TonalityRounded"));
        assert!(page(CustomPage::About, &state).is_none());
    }

    #[test]
    fn levels_snap_to_hundredths_inside_the_range() {
        assert_eq!(value_at(0.0), 0.5);
        assert_eq!(value_at(1.0), 1.5);
        assert_eq!(value_at(0.5), 1.0);
        assert_eq!(value_at(0.4567), 0.96);
        assert_eq!(value_at(2.0), 1.5);
        assert_eq!(
            set("displayGamma", 1.05),
            Change::Config(json!({ "displayGamma": 1.05 }))
        );
    }
}
