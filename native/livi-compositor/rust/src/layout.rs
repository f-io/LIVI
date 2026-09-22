//! Placement math: video planes fill their screen's content region (contain
//! scaling, margins overflow off the output edge), the UI plane fills the
//! screen below the titlebar.

use smithay::utils::{Point, Size};
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::state::{Kind, LiviState};

fn set_size(toplevel: &ToplevelSurface, w: i32, h: i32) {
    toplevel.with_pending_state(|st| {
        st.size = Some(Size::from((w.max(1), h.max(1))));
    });
    toplevel.send_pending_configure();
}

/// Size+position a video plane so its content region fills the screen.
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
        log::info!("video '{}' -> screen '{role}' full {ow}x{oh} at {sx},{sy}", t.tag);
        return;
    }
    // contain the content into the output (uniform scale, bars only on AR mismatch)
    let scale = (ow as f64 / t.vis_w).min(oh as f64 / t.vis_h);
    let off_x = (ow as f64 - t.vis_w * scale) / 2.0;
    let off_y = (oh as f64 - t.vis_h * scale) / 2.0;
    let tw = (t.tier_w * scale).round() as i32;
    let th = (t.tier_h * scale).round() as i32;
    let px = (sx as f64 + off_x - t.crop_l * scale).round() as i32;
    let py = (sy as f64 + off_y - t.crop_t * scale).round() as i32;
    set_size(&t.toplevel, tw, th);
    t.position = Point::from((px, py));
    log::info!("video '{}' -> screen '{role}' {tw}x{th} at {px},{py}", t.tag);
}

/// Apply a cached per-tag config onto its (now present) video toplevel.
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

/// Place the UI plane and titlebar of a screen, ask the client for our size.
pub fn apply_ui_layout(state: &mut LiviState, screen_idx: usize) {
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
            // Tiled on all edges so the client renders exactly our size.
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
