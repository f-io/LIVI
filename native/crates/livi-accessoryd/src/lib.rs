// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
pub mod mgmt;
#[cfg(target_os = "linux")]
pub mod sdp;

#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
pub use server::{Config, NameSource, run};
