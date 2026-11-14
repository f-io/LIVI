// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

pub fn user_data() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("no app container")?;
    Ok(PathBuf::from(home).join("Library/Application Support/LIVI"))
}

// Socket paths stay under the 104 bytes of sun_path: the container sits deep, so the sockets go
// straight into its tmp, named through /var rather than its /private/var target.
pub fn runtime() -> PathBuf {
    let tmp = std::env::temp_dir();
    tmp.strip_prefix("/private")
        .map(|p| PathBuf::from("/").join(p))
        .unwrap_or(tmp)
}
