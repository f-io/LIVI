// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use iap2_mfi::FileCoprocessor;
use iap2_mfi::file::{CERTIFICATE_FILE, IDENTITY_FILE, certificate, identity_major};
use livi_core_proto::message::MfiFile;
use livi_core_proto::state::Mfi;
use sha2::{Digest, Sha256};

fn name(file: MfiFile) -> &'static str {
    match file {
        MfiFile::Certificate => CERTIFICATE_FILE,
        MfiFile::Identity => IDENTITY_FILE,
    }
}

fn both(dir: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    (
        fs::read(dir.join(CERTIFICATE_FILE)).ok(),
        fs::read(dir.join(IDENTITY_FILE)).ok(),
    )
}

pub fn read(dir: &Path) -> Mfi {
    let (cert, key) = both(dir);
    let error = match (&cert, &key) {
        (Some(c), Some(k)) => FileCoprocessor::from_bytes(c, k).err(),
        (Some(c), None) => certificate(c).err(),
        (None, Some(k)) => identity_major(k).err(),
        (None, None) => None,
    };
    Mfi {
        certificate: cert.is_some(),
        identity: key.is_some(),
        generation: key.as_deref().and_then(|k| identity_major(k).ok()),
        error: error.map(|e| e.to_string()),
    }
}

pub fn digest(dir: &Path) -> String {
    let (Some(cert), Some(key)) = both(dir) else {
        return String::new();
    };
    if FileCoprocessor::from_bytes(&cert, &key).is_err() {
        return String::new();
    }
    let sum = Sha256::new()
        .chain_update(&cert)
        .chain_update(&key)
        .finalize();
    sum.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn import(dir: &Path, file: MfiFile, source: &Path) -> Result<(), String> {
    let bytes = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    match file {
        MfiFile::Certificate => certificate(&bytes).map(|_| ()),
        MfiFile::Identity => identity_major(&bytes).map(|_| ()),
    }
    .map_err(|e| e.to_string())?;
    let written = || -> io::Result<()> {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let target = dir.join(name(file));
        let tmp = target.with_extension("new");
        fs::write(&tmp, &bytes)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        fs::rename(&tmp, &target)
    };
    written().map_err(|e| format!("{}: {e}", name(file)))
}

pub fn remove(dir: &Path, file: MfiFile) -> Result<(), String> {
    match fs::remove_file(dir.join(name(file))) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("{}: {e}", name(file))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EC_PEM: &str = include_str!("../../iap2-mfi/src/testdata/ec-p256.pk8.pem");
    const CERT_DER: &[u8] = include_bytes!("../../iap2-mfi/src/testdata/certificate.p7b");

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("livi-core-mfi-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn import_checks_and_stores_privately() {
        let dir = scratch("import");
        let source = dir.join("picked.pk8");
        fs::write(&source, EC_PEM).unwrap();
        let store = dir.join("mfi");
        import(&store, MfiFile::Identity, &source).unwrap();
        let mode = fs::metadata(store.join(IDENTITY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let view = read(&store);
        assert!(view.identity && !view.certificate);
        assert_eq!(view.generation, Some(3));
        assert_eq!(view.error, None);
        assert_eq!(digest(&store), "");
        let cert = dir.join("picked.p7b");
        fs::write(&cert, CERT_DER).unwrap();
        import(&store, MfiFile::Certificate, &cert).unwrap();
        assert!(read(&store).certificate);
        assert_eq!(digest(&store).len(), 16);
        remove(&store, MfiFile::Certificate).unwrap();
        fs::write(&source, b"not a key").unwrap();
        assert!(import(&store, MfiFile::Certificate, &source).is_err());
        assert!(import(&store, MfiFile::Identity, &source).is_err());
        remove(&store, MfiFile::Identity).unwrap();
        remove(&store, MfiFile::Identity).unwrap();
        assert_eq!(read(&store), Mfi::default());
        fs::remove_dir_all(&dir).unwrap();
    }
}
