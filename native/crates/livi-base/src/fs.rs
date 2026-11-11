// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::OsString;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const PRIVATE: u32 = 0o600;
pub const SHARED: u32 = 0o644;

pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>, mode: u32) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let tmp = staging(path);
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode & PRIVATE)
            .open(&tmp)?;
        file.write_all(data.as_ref())?;
        file.set_permissions(Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn staging(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".tmp");
    name.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("livi-base-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_file_lands_with_its_parents_and_its_mode() {
        let dir = Dir::new("parents");
        let path = dir.0.join("a/b/keys");
        write_atomic(&path, "secret", PRIVATE).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "secret");
        assert_eq!(mode_of(&path), PRIVATE);
        assert!(!staging(&path).exists());
    }

    #[test]
    fn a_rewrite_replaces_content_and_mode() {
        let dir = Dir::new("rewrite");
        let path = dir.0.join("config.json");
        write_atomic(&path, "old and longer", SHARED).unwrap();
        write_atomic(&path, "new", 0o755).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(mode_of(&path), 0o755);
    }

    #[test]
    fn a_stale_staging_file_does_not_leak_its_mode() {
        let dir = Dir::new("stale");
        let path = dir.0.join("keys");
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(staging(&path), "left over").unwrap();
        fs::set_permissions(staging(&path), Permissions::from_mode(0o666)).unwrap();
        write_atomic(&path, "fresh", PRIVATE).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh");
        assert_eq!(mode_of(&path), PRIVATE);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file() {
        let dir = Dir::new("failed");
        let path = dir.0.join("state.json");
        write_atomic(&path, "kept", SHARED).unwrap();
        fs::create_dir_all(staging(&path).join("blocker")).unwrap();
        assert!(write_atomic(&path, "lost", SHARED).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "kept");
    }
}
