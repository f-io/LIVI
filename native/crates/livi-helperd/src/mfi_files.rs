// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

use iap2_mfi::FileCoprocessor;
use iap2_mfi::file::{CERTIFICATE_FILE, IDENTITY_FILE};

#[cfg(target_os = "macos")]
fn dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/LIVI/mfi")
}

#[cfg(target_os = "linux")]
fn dir() -> PathBuf {
    crate::linux_main::config_path().with_file_name("mfi")
}

pub fn open_in(dir: &Path) -> Option<FileCoprocessor> {
    if !dir.join(CERTIFICATE_FILE).exists() && !dir.join(IDENTITY_FILE).exists() {
        return None;
    }
    match FileCoprocessor::open(dir) {
        Ok(files) => {
            println!("[helperd] MFi from the imported files in {}", dir.display());
            Some(files)
        }
        Err(e) => {
            println!(
                "[helperd] MFi files in {} are not usable: {e}",
                dir.display()
            );
            None
        }
    }
}

pub fn open() -> Option<FileCoprocessor> {
    open_in(&dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_folder_leaves_the_chip_in_charge() {
        let dir = std::env::temp_dir().join(format!("livi-helperd-mfi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(open_in(&dir).is_none());
        std::fs::write(dir.join(IDENTITY_FILE), b"broken").unwrap();
        assert!(open_in(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
