// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::time::{Duration, Instant};

use livi_core_client::Client;
use livi_core_proto::message::Action;
use livi_core_proto::state::Screen;
use livi_ui_model::dashboard::screens::name;

const SETTLE: Duration = Duration::from_secs(2);

pub fn in_compositor() -> bool {
    std::env::var("LIVI_COMPOSITOR").is_ok_and(|v| v == "1")
}

pub struct Tracked {
    want: bool,
    settle: Option<Instant>,
}

impl Tracked {
    pub fn new(window: &slint::Window, want: bool) -> Self {
        let mut t = Self {
            want: !want,
            settle: None,
        };
        t.apply(window, want);
        t
    }

    pub fn apply(&mut self, window: &slint::Window, want: bool) {
        if self.want == want {
            return;
        }
        self.want = want;
        if window.is_fullscreen() != want {
            window.set_fullscreen(want);
            self.settle = Some(Instant::now() + SETTLE);
        }
    }

    pub fn changed(&mut self, window: &slint::Window) -> Option<bool> {
        let now = window.is_fullscreen();
        if now == self.want {
            self.settle = None;
            return None;
        }
        if self.settle.is_some_and(|t| Instant::now() < t) {
            return None;
        }
        self.want = now;
        self.settle = None;
        Some(now)
    }
}

thread_local! {
    static MAIN: RefCell<Option<Tracked>> = const { RefCell::new(None) };
}

pub fn follow_main(window: &slint::Window, want: bool) {
    if in_compositor() {
        return;
    }
    MAIN.with_borrow_mut(|main| match main {
        Some(t) => t.apply(window, want),
        None => *main = Some(Tracked::new(window, want)),
    });
}

pub fn poll_main(window: &slint::Window, client: &Client) {
    let changed = MAIN.with_borrow_mut(|main| main.as_mut()?.changed(window));
    if let Some(on) = changed {
        save(client, Screen::Main, on);
    }
}

pub fn save(client: &Client, screen: Screen, on: bool) {
    client.act(Action::SetConfig {
        patch: serde_json::json!({ "kiosk": { name(screen): on } }),
    });
}
