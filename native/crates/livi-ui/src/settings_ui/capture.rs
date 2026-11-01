// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) const MODIFIERS: [&str; 8] = [
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "MetaLeft",
    "MetaRight",
];

pub(super) fn stop_capture(ui: &AppWindow) {
    with(|c| c.capturing = None);
    ui.global::<SettingsState>().set_capture_open(false);
}

pub fn capturing() -> bool {
    with(|c| c.capturing.is_some()).unwrap_or(false)
}

pub fn captured(ui: &AppWindow, client: &Client, code: &str) {
    if MODIFIERS.contains(&code) {
        return;
    }
    let binding = with(|c| c.capturing).flatten();
    stop_capture(ui);
    if code == "Escape" {
        return;
    }
    if let Some(binding) = binding {
        client.act(set_binding(binding, code));
    }
}
