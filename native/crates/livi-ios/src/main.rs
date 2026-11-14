// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod gst;
#[cfg(target_os = "ios")]
mod media;
#[cfg(target_os = "ios")]
mod paths;
#[cfg(target_os = "ios")]
mod picker;
#[cfg(target_os = "ios")]
mod planes;
#[cfg(target_os = "ios")]
#[cfg(target_os = "ios")]
mod scene;
#[cfg(target_os = "ios")]
mod screen;

#[cfg(target_os = "ios")]
fn main() {
    if let Err(e) = app::start() {
        eprintln!("[ios] {e}");
    }
}

#[cfg(not(target_os = "ios"))]
fn main() {}
