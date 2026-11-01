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
use slint::winit_030::winit;

use crate::{AppWindow, Page, PhoneState};

const MOVE_EVERY: Duration = Duration::from_millis(16);

thread_local! {
    static FINGERS: RefCell<Fingers> = RefCell::new(Fingers::default());
    static FLUSH_DUE: Cell<bool> = const { Cell::new(false) };
}

fn picture(ui: &AppWindow, x: f64, y: f64) -> Option<(f64, f64)> {
    let size = ui.window().size();
    let (ww, wh) = (f64::from(size.width), f64::from(size.height));
    let (sw, sh) = (
        f64::from(ui.get_stream_width().max(1)),
        f64::from(ui.get_stream_height().max(1)),
    );
    let pw = ww.min(wh * sw / sh);
    let ph = wh.min(ww * sh / sw);
    let (lx, ly) = (x - (ww - pw) / 2.0, y - (wh - ph) / 2.0);
    (pw > 0.0 && ph > 0.0 && (0.0..=pw).contains(&lx) && (0.0..=ph).contains(&ly))
        .then(|| (lx / pw, ly / ph))
}

fn send(client: &Client, points: Vec<Point>) {
    client.input(Input::Pointer {
        screen: Screen::Main,
        points,
    });
}

pub fn handle(ui: &AppWindow, client: &Client, touch: &winit::event::Touch) -> bool {
    let on_picture = ui.get_page() == Page::Projection && ui.get_phone() == PhoneState::Streaming;
    let held = FINGERS.with_borrow(|f| f.holds(touch.id));
    if !on_picture && !held {
        return false;
    }
    let change = match touch.phase {
        winit::event::TouchPhase::Started => Change::Down,
        winit::event::TouchPhase::Moved => Change::Move,
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => Change::Up,
    };
    let change = if on_picture { change } else { Change::Up };
    let at = picture(ui, touch.location.x, touch.location.y);
    if let Some(points) = FINGERS.with_borrow_mut(|f| f.touch(touch.id, at, change)) {
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
