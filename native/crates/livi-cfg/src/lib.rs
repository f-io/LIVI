// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Platform capabilities as cfgs, called from build scripts so a new target opts in here once.
//!
//! - `in_process`: an app process hosts LIVI and hands it its screen, audio and video outputs.
//! - `usb_host`: LIVI may claim USB devices itself.

pub fn emit() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    println!("cargo::rustc-check-cfg=cfg(in_process)");
    println!("cargo::rustc-check-cfg=cfg(usb_host)");
    if matches!(os.as_str(), "android" | "ios") {
        println!("cargo::rustc-cfg=in_process");
    }
    if os != "ios" {
        println!("cargo::rustc-cfg=usb_host");
    }
}
