// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(not(livi_os))]
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
    cfg!(livi_os) || std::env::var("LIVI_KIOSK").is_ok_and(|v| v == "1")
}

/// LIVI OS runs core as root and has no sudo, so core powers the host itself.
#[cfg(livi_os)]
pub fn run(action: Power) {
    println!("[power] host {}", action.arg());
    // SAFETY: sync and reboot take plain values and touch no memory of ours.
    unsafe {
        libc::sync();
        libc::reboot(match action {
            Power::Off => libc::LINUX_REBOOT_CMD_POWER_OFF,
            Power::Reboot => libc::LINUX_REBOOT_CMD_RESTART,
        });
    }
    eprintln!(
        "[power] could not {}: {}",
        action.arg(),
        std::io::Error::last_os_error()
    );
}

#[cfg(not(livi_os))]
pub fn run(action: Power) {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

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
