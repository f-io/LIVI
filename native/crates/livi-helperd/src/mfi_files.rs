// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

use iap2_mfi::FileCoprocessor;

fn dir() -> PathBuf {
    crate::linux_main::config_path().with_file_name("mfi")
}

pub fn open() -> Option<FileCoprocessor> {
    livi_helper::mfi::open_in(&dir())
}
