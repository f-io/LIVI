// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "macos")]
mod control;
#[cfg(target_os = "macos")]
mod main_thread;
#[cfg(target_os = "macos")]
mod screens;
#[cfg(target_os = "macos")]
mod server;

#[cfg(target_os = "macos")]
pub fn serve(control: &std::path::Path, planes: &std::path::Path) -> std::io::Result<()> {
    server::serve(control, planes)
}

#[cfg(target_os = "macos")]
pub fn set_window(screen: &str, ns_view: usize) {
    screens::set_window(screen, ns_view);
}
