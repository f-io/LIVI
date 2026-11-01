// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const POWER_HELPER: &str = "/usr/local/lib/livi/livi-power.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Off,
    Reboot,
}

impl Power {
    fn arg(self) -> &'static str {
        match self {
            Power::Off => "poweroff",
            Power::Reboot => "reboot",
        }
    }
}

pub fn owns_host() -> bool {
    std::env::var("LIVI_KIOSK").is_ok_and(|v| v == "1")
}

pub fn run(action: Power) {
    let action = action.arg();
    if !Path::new(POWER_HELPER).exists() {
        eprintln!("[power] helper missing, run the installer again to let LIVI power the host");
        return;
    }
    let spawned = Command::new("sudo")
        .args(["-n", POWER_HELPER, action])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    match spawned {
        Ok(_) => println!("[power] host {action} requested"),
        Err(e) => eprintln!("[power] could not {action}: {e}"),
    }
}
