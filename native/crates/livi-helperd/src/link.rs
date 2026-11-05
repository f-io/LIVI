// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_runtime::bringup::OnCable;
use livi_runtime::state::HelperState;
use std::sync::Arc;

pub fn drop_dongle_link(mac: String) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = livi_link_host::accessory::drop_link(&mac) {
            eprintln!("[helperd] {mac} stays on the dongle's bluetooth: {e}");
        }
    });
}

pub fn dongle_on_cable(state: Arc<HelperState>) -> OnCable {
    OnCable::dropping(state, |mac| drop_dongle_link(mac.to_string()))
}
