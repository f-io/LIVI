// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::identity::{hex, unhex, write_private};

pub struct Pairings {
    path: PathBuf,
}

impl Pairings {
    pub fn dir(&self) -> Option<&Path> {
        self.path.parent()
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> BTreeMap<String, String> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, controller: &str, public: &[u8]) {
        let mut all = self.load();
        all.insert(controller.to_string(), hex(public));
        let json = serde_json::to_string(&all).expect("strings always serialize");
        if let Err(e) = write_private(&self.path, &json) {
            eprintln!("[cp] could not persist the pairing: {e}");
        }
    }

    pub fn get(&self, controller: &str) -> Option<Vec<u8>> {
        unhex(self.load().get(controller)?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("livi-cp-test-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saved_keys_come_back_by_controller() {
        let dir = TempDir::new();
        let store = Pairings::new(dir.0.join("cp/pairings.json"));
        assert_eq!(store.get("phone-a"), None);
        store.save("phone-a", &[1, 2, 3]);
        store.save("phone-b", &[4]);
        assert_eq!(store.get("phone-a"), Some(vec![1, 2, 3]));
        assert_eq!(store.get("phone-b"), Some(vec![4]));
    }

    #[test]
    fn reads_the_file_the_electron_app_wrote() {
        let dir = TempDir::new();
        let path = dir.0.join("pairings.json");
        fs::write(&path, r#"{"6A2E…":"0aff"}"#).unwrap();
        assert_eq!(Pairings::new(path).get("6A2E…"), Some(vec![0x0a, 0xff]));
    }

    #[test]
    fn a_broken_file_reads_as_empty() {
        let dir = TempDir::new();
        let path = dir.0.join("pairings.json");
        fs::write(&path, "nope").unwrap();
        let store = Pairings::new(path);
        assert_eq!(store.get("x"), None);
        store.save("x", &[9]);
        assert_eq!(store.get("x"), Some(vec![9]));
    }
}
