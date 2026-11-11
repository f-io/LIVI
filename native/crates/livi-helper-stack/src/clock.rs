// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const CLAIM_FILE: &str = "/tmp/livi-gps-clock";
const CLAIM_MAX_AGE_S: u64 = 120;
const STEP_THRESHOLD_S: i64 = 2;

fn gps_owns_clock() -> bool {
    let Ok(meta) = std::fs::metadata(CLAIM_FILE) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|age| age.as_secs() <= CLAIM_MAX_AGE_S)
        .unwrap_or(false)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn step_to(secs: i64) {
    if !cfg!(target_os = "linux") {
        return;
    }
    if gps_owns_clock() {
        println!("[cp] device time: GPS holds the clock, leaving it alone");
        return;
    }
    let offset = secs - now_unix();
    if offset.abs() <= STEP_THRESHOLD_S {
        println!("[cp] device time: offset {offset}s, keeping the system clock");
        return;
    }
    match Command::new(crate::sys::tool("date"))
        .args(["-u", "-s", &format!("@{secs}")])
        .output()
    {
        Ok(out) if out.status.success() => {
            let _ = Command::new(crate::sys::tool("fake-hwclock"))
                .arg("save")
                .output();
            println!("[cp] device time: system clock stepped by {offset}s");
        }
        Ok(out) => eprintln!(
            "[cp] device time: setting the clock failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!("[cp] device time: setting the clock failed: {e}"),
    }
}
