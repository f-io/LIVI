// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use iap2_mfi::FileCoprocessor;
use iap2_mfi::file::{CERTIFICATE_FILE, IDENTITY_FILE};

pub fn open_in(dir: &Path) -> Option<FileCoprocessor> {
    if !dir.join(CERTIFICATE_FILE).exists() && !dir.join(IDENTITY_FILE).exists() {
        return None;
    }
    match FileCoprocessor::open(dir) {
        Ok(files) => {
            println!("[helper] MFi from the imported files in {}", dir.display());
            Some(files)
        }
        Err(e) => {
            println!(
                "[helper] MFi files in {} are not usable: {e}",
                dir.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_folder_leaves_the_chip_in_charge() {
        let dir = std::env::temp_dir().join(format!("livi-helper-mfi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(open_in(&dir).is_none());
        std::fs::write(dir.join(IDENTITY_FILE), b"broken").unwrap();
        assert!(open_in(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
