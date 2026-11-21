// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(any(ui_video, in_process))]
mod main_thread;
#[cfg(ui_video)]
mod planes;
#[cfg(any(ui_video, in_process))]
mod screens;
#[cfg(any(ui_video, in_process))]
mod server;

#[cfg(in_process)]
pub use screens::{View, camera, create, play, remove};

#[cfg(ui_video)]
pub fn serve(control: &std::path::Path, planes: &std::path::Path) -> std::io::Result<()> {
    server::serve(control)?;
    planes::serve(planes)
}

#[cfg(in_process)]
pub fn serve(control: &std::path::Path) -> std::io::Result<()> {
    server::serve(control)
}

#[cfg(any(ui_video, in_process))]
pub fn set_window(screen: &str, window: usize) {
    screens::set_window(screen, window);
}
