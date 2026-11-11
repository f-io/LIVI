// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub mod hci;
mod host;

#[cfg(not(target_os = "linux"))]
pub fn run() -> std::process::ExitCode {
    eprintln!("[btd] linux only");
    std::process::ExitCode::FAILURE
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> std::process::ExitCode {
    eprintln!("[btd] linux only");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{probe, run};
