// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use smithay::utils::{Point, Size};
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::state::{Kind, LiviState, Screen};

fn set_size(toplevel: &ToplevelSurface, w: i32, h: i32) {
    toplevel.with_pending_state(|st| {
        st.size = Some(Size::from((w.max(1), h.max(1))));
    });
    toplevel.send_pending_configure();
}

pub fn apply_video_layout(state: &mut LiviState, idx: usize) {
    let t = &state.toplevels[idx];
    if t.kind != Kind::Video {
        return;
    }
    let s = &state.screens[t.screen_idx];
    let top = s.top_inset();
    let (ow, oh) = (s.width, s.height - top);
    if ow <= 0 || oh <= 0 {
        log::info!(
            "video '{}' deferred: screen '{}' has no window yet",
            state.toplevels[idx].tag,
            s.role
        );
        return;
    }
    let (sx, sy) = (s.x, top);
    let role = s.role.clone();
    let t = &mut state.toplevels[idx];
    if !t.has_crop || t.vis_w <= 0.0 || t.vis_h <= 0.0 || t.tier_w <= 0.0 || t.tier_h <= 0.0 {
        set_size(&t.toplevel, ow, oh);
        t.position = Point::from((sx, sy));
        log::info!(
            "video '{}' -> screen '{role}' full {ow}x{oh} at {sx},{sy}",
            t.tag
        );
        return;
    }
    let scale = (ow as f64 / t.vis_w).min(oh as f64 / t.vis_h);
    let off_x = (ow as f64 - t.vis_w * scale) / 2.0;
    let off_y = (oh as f64 - t.vis_h * scale) / 2.0;
    let tw = (t.tier_w * scale).round() as i32;
    let th = (t.tier_h * scale).round() as i32;
    let px = (sx as f64 + off_x - t.crop_l * scale).round() as i32;
    let py = (sy as f64 + off_y - t.crop_t * scale).round() as i32;
    set_size(&t.toplevel, tw, th);
    t.position = Point::from((px, py));
    log::info!(
        "video '{}' -> screen '{role}' {tw}x{th} at {px},{py}",
        t.tag
    );
}

pub fn apply_cfg_to_video(state: &mut LiviState, tag: &str, idx: usize) {
    let Some(cfg) = state.video_cfgs.iter().find(|c| c.tag == tag).cloned() else {
        return;
    };
    if !cfg.screen.is_empty() {
        if let Some(si) = state.screen_idx_by_role(&cfg.screen) {
            state.toplevels[idx].screen_idx = si;
        }
        let t = &mut state.toplevels[idx];
        t.has_crop = cfg.has_crop;
        t.crop_l = cfg.crop_l;
        t.crop_t = cfg.crop_t;
        t.vis_w = cfg.vis_w;
        t.vis_h = cfg.vis_h;
        t.tier_w = cfg.tier_w;
        t.tier_h = cfg.tier_h;
        apply_video_layout(state, idx);
    }
    if cfg.has_visible {
        state.toplevels[idx].visible = cfg.visible;
    }
    crate::host::damage_all(state);
}

const DEFAULT_FILL_APP_IDS: &str = "retroarch,com.libretro.RetroArch";

pub fn fill_app_ids(env: Option<&str>) -> Vec<String> {
    env.filter(|v| !v.trim().is_empty())
        .unwrap_or(DEFAULT_FILL_APP_IDS)
        .split(',')
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn fills_by_app_id(app_id: Option<&str>, allowed: &[String]) -> bool {
    app_id.is_some_and(|id| allowed.iter().any(|a| *a == id.to_lowercase()))
}

pub fn fill_rect(s: &Screen) -> Option<(i32, i32, i32, i32)> {
    let top = s.top_inset();
    (s.width > 0 && s.height - top > 0).then_some((s.x, top, s.width, s.height - top))
}

// A foreign app that asks for fullscreen (RetroArch) covers LIVI's area of its
// screen, above the UI, which stays mapped beneath it.
pub fn apply_fill_layout(state: &mut LiviState, idx: usize) {
    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgState;
    let t = &state.toplevels[idx];
    if t.kind != Kind::Dialog || !t.fill {
        return;
    }
    let Some((x, y, w, h)) = fill_rect(&state.screens[t.screen_idx]) else {
        return;
    };
    let t = &mut state.toplevels[idx];
    t.position = Point::from((x, y));
    t.toplevel.with_pending_state(|st| {
        st.states.set(XdgState::Fullscreen);
        st.size = Some(Size::from((w, h)));
    });
    if t.toplevel.is_initial_configure_sent() {
        t.toplevel.send_pending_configure();
    }
    crate::host::damage_all(state);
}

pub fn release_fill(state: &mut LiviState, idx: usize) {
    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgState;
    let t = &mut state.toplevels[idx];
    t.fill = false;
    t.toplevel.with_pending_state(|st| {
        st.states.unset(XdgState::Fullscreen);
        st.size = None;
    });
    t.toplevel.send_pending_configure();
}

pub fn apply_ui_layout(state: &mut LiviState, screen_idx: usize) {
    let fills: Vec<usize> = state
        .toplevels
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == Kind::Dialog && t.fill && t.screen_idx == screen_idx)
        .map(|(i, _)| i)
        .collect();
    for idx in fills {
        apply_fill_layout(state, idx);
    }
    let s = &state.screens[screen_idx];
    let (ow, oh) = (s.width, s.height);
    if ow <= 0 || oh <= 0 {
        return;
    }
    let (sx, top) = (s.x, s.top_inset());
    if let Some(ui) = state
        .toplevels
        .iter_mut()
        .find(|t| t.kind == Kind::Ui && t.screen_idx == screen_idx)
    {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgState;
        ui.position = Point::from((sx, top));
        ui.toplevel.with_pending_state(|st| {
            st.states.set(XdgState::TiledTop);
            st.states.set(XdgState::TiledBottom);
            st.states.set(XdgState::TiledLeft);
            st.states.set(XdgState::TiledRight);
            st.size = Some(Size::from((ow.max(1), (oh - top).max(1))));
        });
        ui.toplevel.send_pending_configure();
    }
    crate::host::damage_all(state);
}

pub fn toggle_fullscreen(state: &mut LiviState, screen_idx: usize) {
    let want = !state.screens[screen_idx].fullscreen;
    set_fullscreen(state, screen_idx, want);
}

pub fn set_fullscreen(state: &mut LiviState, screen_idx: usize, want: bool) {
    state.screens[screen_idx].fullscreen = want;
    crate::host::set_fullscreen(state, screen_idx, want);
    if let Some(ui) = state
        .toplevels
        .iter()
        .find(|t| t.kind == Kind::Ui && t.screen_idx == screen_idx)
    {
        ui.toplevel.with_pending_state(|st| {
            if want {
                st.states.set(
                    smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Fullscreen,
                );
            } else {
                st.states.unset(
                    smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Fullscreen,
                );
            }
        });
        ui.toplevel.send_pending_configure();
    }
    apply_ui_layout(state, screen_idx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(x: i32, width: i32, height: i32, fullscreen: bool) -> Screen {
        Screen {
            role: "main".into(),
            x,
            width,
            height,
            req_width: 0,
            req_height: 0,
            fullscreen,
            output: None,
            resize_pending: None,
            applied_width: 0,
            applied_height: 0,
            backdrop_color: [0.0; 4],
            has_backdrop_color: false,
            aspect: None,
        }
    }

    #[test]
    fn retroarch_fills_by_default_and_the_list_can_be_replaced() {
        let ids = fill_app_ids(None);
        assert!(fills_by_app_id(Some("retroarch"), &ids));
        assert!(fills_by_app_id(Some("com.libretro.RetroArch"), &ids));
        assert!(!fills_by_app_id(Some("org.gnome.Nautilus"), &ids));
        assert!(!fills_by_app_id(None, &ids));
        let ids = fill_app_ids(Some(" mpv , Kodi "));
        assert!(fills_by_app_id(Some("kodi"), &ids));
        assert!(!fills_by_app_id(Some("retroarch"), &ids));
        assert_eq!(fill_app_ids(Some("")), fill_app_ids(None));
    }

    #[test]
    fn a_filling_app_covers_the_screen_below_the_title_bar() {
        assert_eq!(
            fill_rect(&screen(100_000, 800, 480, true)),
            Some((100_000, 0, 800, 480))
        );
        assert_eq!(
            fill_rect(&screen(0, 1280, 752, false)),
            Some((0, crate::state::TITLEBAR_H, 1280, 720))
        );
        assert_eq!(fill_rect(&screen(0, 0, 0, true)), None);
    }
}
