// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::{Arc, OnceLock};

use tokio::sync::mpsc;

use crate::server::Core;

#[derive(Debug, Clone, Copy)]
pub enum Key {
    Mute,
    Display,
}

static KEYS: OnceLock<mpsc::UnboundedSender<Key>> = OnceLock::new();

pub fn pressed(key: Key) {
    if let Some(tx) = KEYS.get() {
        let _ = tx.send(key);
    }
}

pub async fn follow(core: Arc<Core>) {
    let (tx, mut keys) = mpsc::unbounded_channel();
    if KEYS.set(tx).is_err() {
        return;
    }
    while let Some(key) = keys.recv().await {
        let mut on = false;
        core.hub.update(|s| {
            let flag = match key {
                Key::Mute => &mut s.system.muted,
                Key::Display => &mut s.system.display_off,
            };
            *flag = !*flag;
            on = *flag;
        });
        println!("[keys] {key:?} {}", if on { "on" } else { "off" });
    }
}
