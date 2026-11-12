// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_base::hex::rgb;
use livi_core_proto::config::{Config, HandDriveType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Projection,
    Telemetry,
    Media,
    Camera,
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

pub fn tabs(config: &Config) -> Vec<Tab> {
    let mut tabs = vec![Tab::Projection];
    if config.dashboard.main {
        tabs.push(Tab::Telemetry);
    }
    if config.media.main {
        tabs.push(Tab::Media);
    }
    if config.camera.main && !config.camera_id.is_empty() {
        tabs.push(Tab::Camera);
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

pub fn look(config: &Config) -> Look {
    let dark = config.dark_mode;
    let pick = |on_dark: &Option<String>, on_light: &Option<String>| {
        if dark { on_dark } else { on_light }
            .as_deref()
            .and_then(rgb)
    };
    let positive = |v: u32, fallback: i32| if v > 0 { v as i32 } else { fallback };
    Look {
        tabs: tabs(config),
        right_hand: config.hand == HandDriveType::Rhd,
        dark,
        primary: pick(&config.primary_color_dark, &config.primary_color_light),
        highlight: pick(&config.highlight_color_dark, &config.highlight_color_light),
        background: pick(
            &config.background_color_dark,
            &config.background_color_light,
        ),
        text: pick(&config.text_color_dark, &config.text_color_light),
        zoom: if config.ui_zoom_percent > 0 {
            config.ui_zoom_percent as f32 / 100.0
        } else {
            1.0
        },
        stream: (
            positive(config.projection_width, 1280),
            positive(config.projection_height, 720),
        ),
        language: config.language.clone(),
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::config::defaults;

    use super::*;

    #[test]
    fn tabs_follow_the_config() {
        let mut config = defaults();
        config.dashboard.main = false;
        config.media.main = true;
        config.camera.main = true;
        config.camera_id.clear();
        assert_eq!(
            tabs(&config),
            vec![Tab::Projection, Tab::Media, Tab::Settings]
        );
        config.dashboard.main = true;
        config.camera_id = "cam0".into();
        assert_eq!(
            tabs(&config),
            vec![
                Tab::Projection,
                Tab::Telemetry,
                Tab::Media,
                Tab::Camera,
                Tab::Settings
            ]
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
        let mut config = defaults();
        config.dark_mode = true;
        config.primary_color_dark = Some("#00ADAD".into());
        config.ui_zoom_percent = 200;
        config.hand = HandDriveType::Rhd;
        let l = look(&config);
        assert_eq!(l.primary, Some([0, 0xad, 0xad]));
        assert_eq!(l.zoom, 2.0);
        assert!(l.right_hand);
    }
}
