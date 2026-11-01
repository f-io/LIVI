// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::WindowBounds;
use livi_core_proto::state::Screen;
use serde_json::Value;

use super::readings::config;

const MIN_SIZE: u32 = 200;
const MAX_SIZE: u32 = 7680;

#[derive(Debug, Clone, PartialEq)]
pub struct Secondary {
    pub screen: Screen,
    pub width: u32,
    pub height: u32,
    pub bounds: Option<WindowBounds>,
    pub kiosk: bool,
    pub dashboard: bool,
}

pub fn name(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "main",
        Screen::Cluster => "cluster",
        Screen::Aux => "aux",
    }
}

pub fn secondary(state: &Value) -> Vec<Secondary> {
    let c = config(state);
    let size = |v: u32| v.clamp(MIN_SIZE, MAX_SIZE);
    [
        (
            Screen::Cluster,
            c.cluster_screen_active,
            c.cluster_screen_width,
            c.cluster_screen_height,
            c.cluster_screen_bounds,
        ),
        (
            Screen::Aux,
            c.aux_screen_active,
            c.aux_screen_width,
            c.aux_screen_height,
            c.aux_screen_bounds,
        ),
    ]
    .into_iter()
    .filter(|(_, active, ..)| *active)
    .map(|(screen, _, w, h, bounds)| Secondary {
        screen,
        width: size(w),
        height: size(h),
        bounds: bounds.filter(|b| b.width >= MIN_SIZE && b.height >= MIN_SIZE),
        kiosk: *c.kiosk.get(screen),
        dashboard: *c.dashboard.get(screen),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use livi_core_proto::config::defaults;
    use serde_json::json;

    use super::*;

    #[test]
    fn only_active_screens_get_a_window() {
        let mut c = serde_json::to_value(defaults()).unwrap();
        c["clusterScreenActive"] = json!(true);
        c["clusterScreenWidth"] = json!(50);
        let s = secondary(&json!({ "config": c }));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].screen, Screen::Cluster);
        assert_eq!(s[0].width, MIN_SIZE);
        assert_eq!(name(s[0].screen), "cluster");
    }
}
