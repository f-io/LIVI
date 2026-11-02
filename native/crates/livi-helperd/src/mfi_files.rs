// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

use iap2_mfi::FileCoprocessor;

#[cfg(target_os = "macos")]
fn dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/LIVI/mfi")
}

#[cfg(target_os = "linux")]
fn dir() -> PathBuf {
    crate::linux_main::config_path().with_file_name("mfi")
}

pub fn open() -> Option<FileCoprocessor> {
    livi_helper::mfi::open_in(&dir())
}
