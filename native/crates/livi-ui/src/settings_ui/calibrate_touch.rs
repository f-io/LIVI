// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;

use livi_core_client::Client;
use livi_core_proto::config::TouchCalibration;
use livi_core_proto::message::Action;
use livi_ui_model::settings::custom::touch::{TARGETS, entry, on_main, solve, store};
use slint::ComponentHandle;

use crate::icons::icon;
use crate::{AppWindow, TouchCalibrationState};

struct Calibrating {
    name: String,
    display: String,
    before: TouchCalibration,
    touched: Vec<(f64, f64)>,
}

thread_local! {
    static CALIBRATING: RefCell<Option<Calibrating>> = const { RefCell::new(None) };
}

fn save(client: &Client, name: &str, entry: &TouchCalibration) {
    client.act(Action::SetConfig {
        patch: store(name, entry),
    });
}

fn show(ui: &AppWindow, step: Option<usize>) {
    let s = ui.global::<TouchCalibrationState>();
    match step.and_then(|i| TARGETS.get(i)) {
        Some(&(x, y)) => {
            s.set_target_x(x as f32);
            s.set_target_y(y as f32);
            s.set_active(true);
        }
        None => s.set_active(false),
    }
}

pub fn start(ui: &AppWindow, client: &Client, n: usize) {
    let state = client.state().clone();
    let Some((name, display)) = on_main(&state, n) else {
        return;
    };
    let before = entry(&state, &name);
    save(
        client,
        &name,
        &TouchCalibration {
            enabled: false,
            ..before.clone()
        },
    );
    CALIBRATING.set(Some(Calibrating {
        name,
        display,
        before,
        touched: Vec::new(),
    }));
    show(ui, Some(0));
}

fn finish(ui: &AppWindow, client: &Client, matrix: Option<[f64; 6]>) {
    let Some(c) = CALIBRATING.take() else {
        return;
    };
    let after = match matrix {
        Some(matrix) => TouchCalibration {
            display: c.display,
            enabled: true,
            matrix,
        },
        None => c.before,
    };
    save(client, &c.name, &after);
    show(ui, None);
}

pub fn wire(ui: &AppWindow, client: &Client) {
    let s = ui.global::<TouchCalibrationState>();
    s.set_close_icon(icon("Close").unwrap_or_default());
    s.on_tap({
        let (weak, client) = (ui.as_weak(), client.clone());
        move |u, v| {
            let Some(ui) = weak.upgrade() else { return };
            let done = CALIBRATING.with_borrow_mut(|c| {
                let c = c.as_mut()?;
                c.touched.push((f64::from(u), f64::from(v)));
                Some(c.touched.len())
            });
            match done {
                Some(n) if n < TARGETS.len() => show(&ui, Some(n)),
                Some(_) => {
                    let matrix =
                        CALIBRATING.with_borrow(|c| c.as_ref().and_then(|c| solve(&c.touched)));
                    finish(&ui, &client, matrix);
                }
                None => show(&ui, None),
            }
        }
    });
    s.on_cancel({
        let (weak, client) = (ui.as_weak(), client.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                finish(&ui, &client, None);
            }
        }
    });
}
