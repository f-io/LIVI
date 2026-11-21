// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Platform capabilities as cfgs, called from build scripts so a new target opts in here once.
//!
//! - `in_process`: an app process hosts LIVI and hands it its screen, audio and video outputs.
//! - `usb_host`: LIVI may claim USB devices itself.
//! - `ui_video`: the UI process draws the video planes.

pub fn emit() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    println!("cargo::rustc-check-cfg=cfg(in_process)");
    println!("cargo::rustc-check-cfg=cfg(usb_host)");
    println!("cargo::rustc-check-cfg=cfg(ui_video)");
    let livi_os = os == "linux" && env == "musl";
    if os == "macos" || livi_os {
        println!("cargo::rustc-cfg=ui_video");
    }
    if matches!(os.as_str(), "android" | "ios") {
        println!("cargo::rustc-cfg=in_process");
    }
    if os != "ios" {
        println!("cargo::rustc-cfg=usb_host");
    }
}
