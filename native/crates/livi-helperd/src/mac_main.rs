// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

use livi_core_proto::helper::{HelperSockets, USER_DATA};
use livi_helper::embedded::Host;

fn user_data() -> PathBuf {
    if let Some(dir) = std::env::var_os(USER_DATA).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/LIVI")
}

pub fn host() -> Host {
    let user_data = user_data();
    Host {
        sockets: HelperSockets::system(),
        mfi: user_data.join("mfi"),
        lockdown: crate::lockdown_dir(),
        bt_keys: user_data.join("bluetooth-keys"),
    }
}
