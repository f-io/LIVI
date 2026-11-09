// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::{ViewArea, WindowBounds};
use livi_core_proto::state::{Screen, State};

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
    pub view_area: ViewArea,
}

pub fn name(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "main",
        Screen::Cluster => "cluster",
        Screen::Aux => "aux",
    }
}

pub fn secondary(state: &State) -> Vec<Secondary> {
    let c = &state.config;
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
        view_area: *c.view_area.get(screen),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_screens_get_a_window() {
        let mut state = State::default();
        state.config.cluster_screen_active = true;
        state.config.cluster_screen_width = 50;
        state.config.aux_screen_active = false;
        let s = secondary(&state);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].screen, Screen::Cluster);
        assert_eq!(s[0].width, MIN_SIZE);
        assert_eq!(name(s[0].screen), "cluster");
    }

    #[test]
    fn each_window_gets_its_own_view_area() {
        let mut state = State::default();
        state.config.cluster_screen_active = true;
        state.config.aux_screen_active = true;
        state.config.view_area.cluster.left = 40;
        state.config.view_area.aux.bottom = 12;
        state.config.cluster_view_area_left = 300;
        let s = secondary(&state);
        assert_eq!(s[0].view_area.left, 40);
        assert_eq!(s[0].view_area.bottom, 0);
        assert_eq!(s[1].view_area.bottom, 12);
        assert_eq!(s[1].view_area.left, 0);
    }
}
