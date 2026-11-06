// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

use livi_core_proto::helper::HelperSockets;
use livi_helper::embedded::Host;

pub fn host() -> Host {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    Host {
        sockets: HelperSockets::system(),
        mfi: home.join("Library/Application Support/LIVI/mfi"),
        lockdown: crate::lockdown_dir(),
        bt_keys: home.join(".config/LIVI/bluetooth-keys"),
    }
}
