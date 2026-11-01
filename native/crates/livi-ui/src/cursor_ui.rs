// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::Cell;
use std::time::Duration;

use slint::ComponentHandle;
#[cfg(not(target_os = "macos"))]
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::dpi::PhysicalPosition;

use crate::AppWindow;

const HIDE_AFTER: Duration = Duration::from_secs(3);

thread_local! {
    static LAST: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
    static SHOWN: Cell<bool> = const { Cell::new(true) };
    static IDLE: slint::Timer = slint::Timer::default();
}

#[cfg(not(target_os = "macos"))]
fn visible(ui: &AppWindow, on: bool) {
    let applied = ui.window().with_winit_window(|w| w.set_cursor_visible(on));
    if applied.is_some() {
        SHOWN.set(on);
    }
}

#[cfg(target_os = "macos")]
fn visible(_ui: &AppWindow, on: bool) {
    if !on {
        objc2_app_kit::NSCursor::setHiddenUntilMouseMoves(true);
    }
    SHOWN.set(on);
}

pub fn start(ui: &AppWindow) {
    visible(ui, false);
}

pub fn moved(ui: &AppWindow, position: PhysicalPosition<f64>) {
    let now = (position.x, position.y);
    let before = LAST.replace(Some(now));
    if before.is_none_or(|b| b == now) {
        return;
    }
    if !SHOWN.get() {
        visible(ui, true);
    }
    let weak = ui.as_weak();
    IDLE.with(|t| {
        t.start(slint::TimerMode::SingleShot, HIDE_AFTER, move || {
            if let Some(ui) = weak.upgrade() {
                visible(&ui, false);
            }
        })
    });
}
