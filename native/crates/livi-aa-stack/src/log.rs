// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

pub fn set_debug(enabled: bool) {
    DEBUG.store(enabled, Ordering::Relaxed);
}

pub fn debug() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    DEBUG.load(Ordering::Relaxed) || *ENV.get_or_init(|| env_on("DEBUG"))
}

macro_rules! detail {
    ($($arg:tt)*) => {
        if $crate::log::debug() {
            println!($($arg)*);
        }
    };
}
pub(crate) use detail;

pub fn trace() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    *ENV.get_or_init(|| env_on("TRACE"))
}

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
