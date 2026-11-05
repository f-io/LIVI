// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::process::ExitCode;

use livi_core_proto::helper::{HelperSockets, HelperStart};
use livi_helper::embedded::{Embedded, Host};

fn host() -> Host {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    Host {
        sockets: HelperSockets::system(),
        mfi: home.join("Library/Application Support/LIVI/mfi"),
        lockdown: crate::lockdown_dir(),
        bt_keys: home.join(".config/LIVI/bluetooth-keys"),
    }
}

pub fn run() -> ExitCode {
    let helper = match Embedded::start(HelperStart::from_env(), host()) {
        Ok(helper) => helper,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let signals = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    signals.block_on(crate::shutdown_signal());
    println!("[helperd] shutting down");
    drop(helper);
    livi_runtime::bonjour::stop();
    ExitCode::SUCCESS
}
