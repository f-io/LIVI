// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_client::Client;
use livi_core_proto::message::Action;
use livi_core_proto::state::VideoPlayback;
use slint::ComponentHandle;

use crate::{AppWindow, Page, PhoneState};

pub fn show(ui: &AppWindow, video: Option<VideoPlayback>) {
    ui.set_video_shown(video.is_some());
    let v = video.unwrap_or_default();
    ui.set_video_playing(v.playing);
    ui.set_video_elapsed_ms(v.position_ms as f32);
    ui.set_video_total_ms(v.duration_ms.unwrap_or(0.0) as f32);
    crate::media_ui::follow(ui);
}

pub fn mirrored(ui: &AppWindow, mirrored: bool) {
    ui.set_mirrored(mirrored);
    if mirrored {
        wake_mirror(ui);
    }
}

pub fn mirror_on_screen(ui: &AppWindow) -> bool {
    ui.get_mirrored()
        && !ui.get_video_shown()
        && ui.get_page() == Page::Projection
        && ui.get_phone() == PhoneState::Streaming
}

pub fn wake_mirror(ui: &AppWindow) {
    ui.set_mirror_shown(true);
    ui.set_mirror_idle(0);
}

pub fn leave_mirror(ui: &AppWindow) {
    ui.set_mirror_shown(false);
    crate::choose(ui, Page::Media);
}

pub fn wire(ui: &AppWindow, client: &Client) {
    let weak = ui.as_weak();
    ui.on_mirror_leave({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                leave_mirror(&ui);
            }
        }
    });
    ui.on_video_play_pause({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                c.act(Action::VideoPlay {
                    playing: !ui.get_video_playing(),
                });
            }
        }
    });
    ui.on_video_seek({
        let c = client.clone();
        move |share| {
            if let Some(ui) = weak.upgrade() {
                let at = f64::from(ui.get_video_total_ms()) * f64::from(share);
                c.act(Action::VideoSeek {
                    position_ms: at.round() as u32,
                });
            }
        }
    });
    ui.on_video_close({
        let c = client.clone();
        move || c.act(Action::VideoClose)
    });
}
