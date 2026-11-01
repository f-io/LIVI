// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Projection,
    Telemetry,
    Media,
    Camera,
    Games,
    Settings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub tabs: Vec<Tab>,
    pub right_hand: bool,
    pub dark: bool,
    pub primary: Option<[u8; 3]>,
    pub highlight: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub text: Option<[u8; 3]>,
    pub zoom: f32,
    pub stream: (i32, i32),
    pub language: String,
}

fn on_main(config: &Value, key: &str, default: bool) -> bool {
    config[key]["main"].as_bool().unwrap_or(default)
}

pub fn hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn tabs(config: &Value) -> Vec<Tab> {
    let camera_ready = config["cameraId"].as_str().is_some_and(|id| !id.is_empty());
    let mut tabs = vec![Tab::Projection];
    if on_main(config, "dashboard", false) {
        tabs.push(Tab::Telemetry);
    }
    if on_main(config, "media", true) {
        tabs.push(Tab::Media);
    }
    if on_main(config, "camera", true) && camera_ready {
        tabs.push(Tab::Camera);
    }
    if config["games"]["enabled"].as_bool().unwrap_or(false) {
        tabs.push(Tab::Games);
    }
    tabs.push(Tab::Settings);
    tabs
}

pub const MIN_UI_HEIGHT: f32 = 292.0;

pub fn ui_scale(base: f32, zoom: f32, physical_height: f32) -> f32 {
    let height = physical_height / base.max(f32::EPSILON);
    let fit = if height > 0.0 {
        (height / MIN_UI_HEIGHT).min(1.0)
    } else {
        1.0
    };
    base * zoom * fit
}

pub fn look(config: &Value) -> Look {
    let dark = config["darkMode"].as_bool().unwrap_or(true);
    let pick = |dark_key: &str, light_key: &str| {
        config[if dark { dark_key } else { light_key }]
            .as_str()
            .and_then(hex_color)
    };
    let w = config["projectionWidth"]
        .as_i64()
        .filter(|v| *v > 0)
        .unwrap_or(1280) as i32;
    let h = config["projectionHeight"]
        .as_i64()
        .filter(|v| *v > 0)
        .unwrap_or(720) as i32;
    Look {
        tabs: tabs(config),
        right_hand: config["hand"].as_i64() == Some(1),
        dark,
        primary: pick("primaryColorDark", "primaryColorLight"),
        highlight: pick("highlightColorDark", "highlightColorLight"),
        background: pick("backgroundColorDark", "backgroundColorLight"),
        text: pick("textColorDark", "textColorLight"),
        zoom: config["uiZoomPercent"]
            .as_f64()
            .filter(|z| *z > 0.0)
            .map_or(1.0, |z| (z / 100.0) as f32),
        stream: (w, h),
        language: config["language"].as_str().unwrap_or("en").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tabs_follow_the_config() {
        assert_eq!(
            tabs(&json!({})),
            vec![Tab::Projection, Tab::Media, Tab::Settings]
        );
        assert_eq!(
            tabs(&json!({ "dashboard": { "main": true }, "cameraId": "cam0" })),
            vec![
                Tab::Projection,
                Tab::Telemetry,
                Tab::Media,
                Tab::Camera,
                Tab::Settings
            ]
        );
        assert_eq!(
            tabs(&json!({ "games": { "enabled": true } })),
            vec![Tab::Projection, Tab::Media, Tab::Games, Tab::Settings]
        );
    }

    #[test]
    fn the_scale_zooms_and_shrinks_below_the_minimum_height() {
        assert_eq!(ui_scale(2.0, 1.0, 1440.0), 2.0);
        assert_eq!(ui_scale(1.0, 1.5, 1440.0), 1.5);
        assert_eq!(ui_scale(1.0, 0.8, 720.0), 0.8);
        assert_eq!(ui_scale(1.0, 1.0, 146.0), 0.5);
        assert_eq!(ui_scale(2.0, 1.0, 292.0), 1.0);
        assert_eq!(ui_scale(1.0, 1.0, 0.0), 1.0);
    }

    #[test]
    fn colors_and_zoom() {
        let l = look(
            &json!({ "darkMode": true, "primaryColorDark": "#00ADAD", "uiZoomPercent": 200, "hand": 1 }),
        );
        assert_eq!(l.primary, Some([0, 0xad, 0xad]));
        assert_eq!(l.zoom, 2.0);
        assert!(l.right_hand);
        assert_eq!(hex_color("nope"), None);
    }
}
