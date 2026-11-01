// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use livi_ui_model::overlay::{VolumeIcon, percent, volume_icon};
use slint::{ComponentHandle, Timer, TimerMode};

use crate::icons::icon;
use crate::{AppWindow, OverlayState, Page};

const LEVEL_SHOWN: Duration = Duration::from_millis(1500);
const PHONE_SHOWN: Duration = Duration::from_millis(1170);

thread_local! {
    static LEVEL_HIDE: Timer = Timer::default();
    static PHONE_HIDE: Timer = Timer::default();
}

fn hide_later(
    timer: &'static std::thread::LocalKey<Timer>,
    ui: &AppWindow,
    after: Duration,
    hide: fn(&OverlayState),
) {
    let weak = ui.as_weak();
    timer.with(|t| {
        t.start(TimerMode::SingleShot, after, move || {
            if let Some(ui) = weak.upgrade() {
                hide(&ui.global::<OverlayState>());
            }
        })
    });
}

pub fn volume(ui: &AppWindow, level: f64) {
    if ui.get_page() == Page::Settings {
        return;
    }
    let name = match volume_icon(level) {
        VolumeIcon::Off => "VolumeOffRounded",
        VolumeIcon::Down => "VolumeDownRounded",
        VolumeIcon::Up => "VolumeUpRounded",
    };
    let o = ui.global::<OverlayState>();
    if let Some(img) = icon(name) {
        o.set_level_icon(img);
    }
    o.set_level_percent(percent(level) as i32);
    o.set_level_shown(true);
    hide_later(&LEVEL_HIDE, ui, LEVEL_SHOWN, |o| o.set_level_shown(false));
}

pub fn phone(ui: &AppWindow, name: &str) {
    let o = ui.global::<OverlayState>();
    if let Some(img) = icon("PhoneIphoneRounded") {
        o.set_phone_icon(img);
    }
    o.set_phone_name(name.into());
    o.set_phone_shown(true);
    hide_later(&PHONE_HIDE, ui, PHONE_SHOWN, |o| o.set_phone_shown(false));
}
