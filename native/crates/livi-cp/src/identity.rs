// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::crypto::{ed25519_public, random_bytes};

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub secret: [u8; 32],
    pub public: [u8; 32],
    pub pairing_id: String,
}

impl Identity {
    pub fn pk_hex(&self) -> String {
        hex(&self.public)
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(rename = "priv")]
    secret: String,
    #[serde(rename = "pub")]
    public: String,
    pi: String,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn read(path: &Path) -> Option<Identity> {
    let stored: Stored = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some(Identity {
        secret: unhex(&stored.secret)?.try_into().ok()?,
        public: unhex(&stored.public)?.try_into().ok()?,
        pairing_id: stored.pi,
    })
}

pub(crate) fn write_private(path: &Path, data: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let _ = fs::remove_file(&tmp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    io::Write::write_all(&mut file, data.as_bytes())?;
    fs::rename(&tmp, path)
}

pub fn load_or_create(path: &Path) -> Identity {
    if let Some(id) = read(path) {
        return id;
    }
    let secret = random_bytes::<32>();
    let id = Identity {
        secret,
        public: ed25519_public(&secret),
        pairing_id: uuid::Uuid::new_v4().to_string(),
    };
    let stored = Stored {
        secret: hex(&id.secret),
        public: hex(&id.public),
        pi: id.pairing_id.clone(),
    };
    let json = serde_json::to_string(&stored).expect("strings always serialize");
    if let Err(e) = write_private(path, &json) {
        eprintln!("[cp] could not persist the identity: {e}");
    }
    id
}

const PLACEHOLDERS: [&str; 3] = [
    "00:00:00:00:00:00",
    "02:00:00:00:00:00",
    "AA:BB:CC:DD:EE:FF",
];

fn as_mac(text: &str) -> Option<String> {
    let mac = text.trim().to_uppercase();
    let parts: Vec<&str> = mac.split(':').collect();
    let well_formed = parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()));
    (well_formed && !PLACEHOLDERS.contains(&mac.as_str())).then_some(mac)
}

pub fn derived_accessory_id(pairing_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(pairing_id.as_bytes());
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&digest[..6]);
    mac[0] = (mac[0] & 0xfc) | 0x02;
    mac.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn accessory_id(
    path: &Path,
    pairing_id: &str,
    hardware: impl FnOnce() -> Option<String>,
) -> String {
    if let Some(id) = fs::read_to_string(path).ok().and_then(|s| as_mac(&s)) {
        return id;
    }
    let id = hardware()
        .and_then(|m| as_mac(&m))
        .unwrap_or_else(|| derived_accessory_id(pairing_id));
    match write_private(path, &format!("{id}\n")) {
        Ok(()) => println!("[cp] the accessory is {id} from now on"),
        Err(e) => eprintln!("[cp] could not keep the accessory id: {e}"),
    }
    id
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::pairings::tests::TempDir;

    #[test]
    fn created_once_then_read_back() {
        let dir = TempDir::new();
        let path = dir.0.join("cp/identity.json");
        let first = load_or_create(&path);
        assert_eq!(first.public, ed25519_public(&first.secret));
        assert_eq!(first.pk_hex().len(), 64);
        assert_eq!(load_or_create(&path), first);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn reads_the_file_the_electron_app_wrote() {
        let dir = TempDir::new();
        let path = dir.0.join("identity.json");
        let secret = [7u8; 32];
        let public = ed25519_public(&secret);
        let json = format!(
            r#"{{"priv":"{}","pub":"{}","pi":"abc"}}"#,
            hex(&secret),
            hex(&public)
        );
        fs::write(&path, json).unwrap();
        assert_eq!(
            load_or_create(&path),
            Identity {
                secret,
                public,
                pairing_id: "abc".into()
            }
        );
    }

    #[test]
    fn a_broken_file_starts_over() {
        let dir = TempDir::new();
        let path = dir.0.join("identity.json");
        fs::write(&path, r#"{"priv":"zz","pub":"00","pi":"x"}"#).unwrap();
        assert_ne!(load_or_create(&path).pairing_id, "x");
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 0xab, 0xff])), Some(vec![0, 0xab, 0xff]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn the_accessory_keeps_the_first_address_it_was_given() {
        let dir = TempDir::new();
        let path = dir.0.join("cp/accessory-id");
        let first = accessory_id(&path, "pi", || Some("88:00:33:77:8c:26".into()));
        assert_eq!(first, "88:00:33:77:8C:26");
        assert_eq!(
            accessory_id(&path, "pi", || Some("11:22:33:44:55:66".into())),
            first
        );
        assert_eq!(accessory_id(&path, "pi", || None), first);
    }

    #[test]
    fn without_hardware_the_accessory_comes_from_the_pairing_id() {
        let dir = TempDir::new();
        let path = dir.0.join("accessory-id");
        let id = accessory_id(&path, "pi", || Some("02:00:00:00:00:00".into()));
        assert_eq!(id, derived_accessory_id("pi"));
        assert_ne!(derived_accessory_id("pi"), derived_accessory_id("other"));
        let first = u8::from_str_radix(&id[..2], 16).unwrap();
        assert_eq!(first & 0x03, 0x02);
        assert_eq!(as_mac(&id), Some(id.clone()));
        assert_eq!(accessory_id(&path, "other", || None), id);
    }
}
