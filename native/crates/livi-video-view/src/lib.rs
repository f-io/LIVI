// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(any(target_os = "macos", target_os = "android"))]
mod control;
#[cfg(any(target_os = "macos", target_os = "android"))]
mod main_thread;
#[cfg(target_os = "macos")]
mod planes;
#[cfg(any(target_os = "macos", target_os = "android"))]
mod screens;
#[cfg(any(target_os = "macos", target_os = "android"))]
mod server;

#[cfg(target_os = "android")]
pub use screens::{View, camera, create, play, remove};

#[cfg(target_os = "macos")]
pub fn serve(control: &std::path::Path, planes: &std::path::Path) -> std::io::Result<()> {
    server::serve(control)?;
    planes::serve(planes)
}

#[cfg(target_os = "android")]
pub fn serve(control: &std::path::Path) -> std::io::Result<()> {
    server::serve(control)
}

#[cfg(any(target_os = "macos", target_os = "android"))]
pub fn set_window(screen: &str, window: usize) {
    screens::set_window(screen, window);
}
