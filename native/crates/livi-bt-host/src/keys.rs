// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;
use std::path::PathBuf;

use livi_base::fs::{PRIVATE, write_atomic};
use livi_base::hex::{decode as unhex, encode as hex};

use crate::hci::{self, Addr};

pub struct Keys {
    path: PathBuf,
    keys: BTreeMap<Addr, ([u8; 16], u8)>,
}

impl Keys {
    pub fn load(path: PathBuf) -> Self {
        let mut keys = BTreeMap::new();
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            let mut parts = line.split_whitespace();
            let (Some(addr), Some(key), Some(kind)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Some(addr), Some(key), Ok(kind)) =
                (hci::parse_addr(addr), unhex(key), kind.parse::<u8>())
            else {
                continue;
            };
            if let Ok(key) = key.try_into() {
                keys.insert(addr, (key, kind));
            }
        }
        Self { path, keys }
    }

    pub fn get(&self, addr: &Addr) -> Option<[u8; 16]> {
        self.keys.get(addr).map(|(key, _)| *key)
    }

    pub fn bonded(&self) -> Vec<Addr> {
        self.keys.keys().copied().collect()
    }

    pub fn put(&mut self, addr: Addr, key: [u8; 16], kind: u8) {
        self.keys.insert(addr, (key, kind));
        self.save();
    }

    pub fn forget(&mut self, addr: &Addr) {
        if self.keys.remove(addr).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        let text: String = self
            .keys
            .iter()
            .map(|(addr, (key, kind))| format!("{} {} {kind}\n", hci::text(addr), hex(key)))
            .collect();
        if let Err(e) = write_atomic(&self.path, text, PRIVATE) {
            eprintln!(
                "[bt] could not keep the link keys in {}: {e}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_outlive_a_restart_and_can_be_forgotten() {
        let dir = std::env::temp_dir().join(format!("livi-bt-keys-{}", std::process::id()));
        let path = dir.join("keys");
        let phone = hci::parse_addr("94:45:60:A2:1B:BA").unwrap();
        let mut keys = Keys::load(path.clone());
        assert_eq!(keys.get(&phone), None);
        keys.put(phone, [7; 16], 4);
        let again = Keys::load(path.clone());
        assert_eq!(again.get(&phone), Some([7; 16]));
        assert_eq!(again.bonded(), vec![phone]);
        keys.forget(&phone);
        assert_eq!(Keys::load(path).get(&phone), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
