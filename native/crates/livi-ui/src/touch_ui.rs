// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::{Cell, RefCell};
use std::time::Duration;

use livi_core_client::Client;
use livi_core_proto::input::{Input, Point};
use livi_core_proto::state::Screen;
use livi_ui_model::touch::{Change, Fingers};
use slint::ComponentHandle;
#[cfg(not(target_os = "android"))]
use slint::winit_030::winit;

use crate::{AppWindow, Page, PhoneState};

const MOVE_EVERY: Duration = Duration::from_millis(16);

thread_local! {
    static FINGERS: RefCell<Fingers> = RefCell::new(Fingers::default());
    static FLUSH_DUE: Cell<bool> = const { Cell::new(false) };
}

fn picture(ui: &AppWindow, x: f64, y: f64) -> Option<(f64, f64)> {
    let scale = f64::from(ui.window().scale_factor());
    let (left, top) = (
        f64::from(ui.get_content_left()) * scale,
        f64::from(ui.get_content_top()) * scale,
    );
    let (ww, wh) = (
        f64::from(ui.get_content_width()) * scale,
        f64::from(ui.get_content_height()) * scale,
    );
    let (sw, sh) = (
        f64::from(ui.get_stream_width().max(1)),
        f64::from(ui.get_stream_height().max(1)),
    );
    let pw = ww.min(wh * sw / sh);
    let ph = wh.min(ww * sh / sw);
    let (lx, ly) = (x - left - (ww - pw) / 2.0, y - top - (wh - ph) / 2.0);
    (pw > 0.0 && ph > 0.0 && (0.0..=pw).contains(&lx) && (0.0..=ph).contains(&ly))
        .then(|| (lx / pw, ly / ph))
}

fn send(client: &Client, points: Vec<Point>) {
    client.input(Input::Pointer {
        screen: Screen::Main,
        points,
    });
}

#[cfg(not(target_os = "android"))]
pub fn winit(ui: &AppWindow, client: &Client, touch: &winit::event::Touch) -> bool {
    let change = match touch.phase {
        winit::event::TouchPhase::Started => Change::Down,
        winit::event::TouchPhase::Moved => Change::Move,
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => Change::Up,
    };
    handle(
        ui,
        client,
        touch.id,
        (touch.location.x, touch.location.y),
        change,
    )
}

#[cfg(target_os = "android")]
pub fn wire(ui: &AppWindow, client: &Client) {
    use i_slint_backend_android_activity::{Touch, set_touch_hook};

    let weak = ui.as_weak();
    let client = client.clone();
    set_touch_hook(move |id, x, y, phase| {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        if phase == Touch::Started {
            crate::keys_ui::touched(&ui);
        }
        let change = match phase {
            Touch::Started => Change::Down,
            Touch::Moved => Change::Move,
            Touch::Ended | Touch::Cancelled => Change::Up,
        };
        let finger = u64::try_from(id).unwrap_or_default();
        handle(&ui, &client, finger, (f64::from(x), f64::from(y)), change)
    });
}

fn handle(
    ui: &AppWindow,
    client: &Client,
    finger: u64,
    (x, y): (f64, f64),
    change: Change,
) -> bool {
    let on_picture = ui.get_page() == Page::Projection
        && ui.get_phone() == PhoneState::Streaming
        && !ui.get_video_shown();
    let held = FINGERS.with_borrow(|f| f.holds(finger));
    if !on_picture && !held {
        return false;
    }
    let change = if on_picture { change } else { Change::Up };
    let at = picture(ui, x, y);
    if let Some(points) = FINGERS.with_borrow_mut(|f| f.touch(finger, at, change)) {
        send(client, points);
    } else if change == Change::Move && !FLUSH_DUE.replace(true) {
        let client = client.clone();
        slint::Timer::single_shot(MOVE_EVERY, move || {
            FLUSH_DUE.set(false);
            if let Some(points) = FINGERS.with_borrow_mut(Fingers::flush) {
                send(&client, points);
            }
        });
    }
    true
}
