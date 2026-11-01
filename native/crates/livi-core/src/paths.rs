// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(not(target_os = "macos"))]
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use livi_core_proto::runtime::runtime_dir;

pub struct Paths {
    pub config: PathBuf,
    pub config_backup: PathBuf,
    pub runtime: PathBuf,
}

impl Paths {
    pub fn from_env(uid: u32) -> Result<Self, String> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set")?;
        let user_data = user_data_dir(&home);
        Ok(Self {
            config: user_data.join("config.json"),
            config_backup: backup_dir(&home, &user_data).join("config.json"),
            runtime: runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"), uid),
        })
    }

    pub fn socket(&self) -> PathBuf {
        self.runtime.join("core.sock")
    }

    pub fn lock(&self) -> PathBuf {
        self.runtime.join("core.lock")
    }
}

#[cfg(target_os = "macos")]
fn user_data_dir(home: &Path) -> PathBuf {
    home.join("Library/Application Support/LIVI")
}

#[cfg(not(target_os = "macos"))]
fn user_data_dir(home: &Path) -> PathBuf {
    xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), home, ".config").join("LIVI")
}

#[cfg(target_os = "macos")]
fn backup_dir(_home: &Path, user_data: &Path) -> PathBuf {
    user_data.join("backup")
}

#[cfg(not(target_os = "macos"))]
fn backup_dir(home: &Path, _user_data: &Path) -> PathBuf {
    xdg_dir(std::env::var_os("XDG_DATA_HOME"), home, ".local/share").join("LIVI")
}

#[cfg(not(target_os = "macos"))]
fn xdg_dir(value: Option<OsString>, home: &Path, fallback: &str) -> PathBuf {
    value
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(fallback))
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn xdg_dirs_fall_back_to_home() {
        let home = Path::new("/home/u");
        assert_eq!(
            xdg_dir(Some("/cfg".into()), home, ".config"),
            PathBuf::from("/cfg")
        );
        assert_eq!(
            xdg_dir(Some("".into()), home, ".config"),
            PathBuf::from("/home/u/.config")
        );
        assert_eq!(
            xdg_dir(None, home, ".local/share"),
            PathBuf::from("/home/u/.local/share")
        );
    }
}
