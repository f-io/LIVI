// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod carkit;
pub use carkit::{CarkitChannel, LOCKDOWN_SERVICE, open_carkit, pair_record_path};

#[cfg(unix)]
pub mod usbmuxd;

#[cfg(target_os = "macos")]
pub mod mac_network;
