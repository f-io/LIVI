// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use livi_core_proto::state::DabStation;
use livi_dab::Ensemble;

pub fn labels_complete(e: &Ensemble) -> bool {
    e.label.is_some()
        && e.dab_plus_services().count() > 0
        && e.dab_plus_services().all(|(s, _)| s.label.is_some())
}

pub fn stations(e: &Ensemble, channel: &str, frequency: u32) -> Vec<DabStation> {
    let ensemble = e.label.clone().unwrap_or_default();
    e.dab_plus_services()
        .filter_map(|(s, _)| {
            Some(DabStation {
                service_id: s.id,
                label: s.label.clone()?,
                short_label: s.short_label.clone().unwrap_or_default(),
                ensemble: ensemble.clone(),
                channel: channel.to_string(),
                frequency_hz: frequency,
                favorite: false,
                hidden: false,
                logo: None,
            })
        })
        .collect()
}

pub const LOGOS: &str = "dab/logos";

fn image_path(dir: &Path, service: u32) -> Option<PathBuf> {
    ["jpg", "png"]
        .iter()
        .map(|ext| dir.join(format!("{service:08x}.{ext}")))
        .find(|p| p.exists())
}

pub fn keep_image(dir: &Path, service: u32, mime: &str, data: &[u8]) {
    let ext = if mime == "image/png" { "png" } else { "jpg" };
    if let Some(old) = image_path(dir, service) {
        let _ = std::fs::remove_file(old);
    }
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join(format!("{service:08x}.{ext}")), data);
    }
}

pub fn load_images(dir: &Path) -> HashMap<u32, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let service = u32::from_str_radix(path.file_stem()?.to_str()?, 16).ok()?;
            let data = std::fs::read(&path).ok()?;
            Some((
                service,
                base64::engine::general_purpose::STANDARD.encode(data),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_dab::fic::{Component, Protection, Service, Subchannel};

    fn ensemble() -> Ensemble {
        let mut e = Ensemble {
            label: Some("DR Deutschland".into()),
            ..Default::default()
        };
        e.subchannels.insert(
            11,
            Subchannel {
                id: 11,
                start: 616,
                size: 104,
                protection: Protection::Eep {
                    option: 0,
                    level: 1,
                },
            },
        );
        e.services.insert(
            0xd220,
            Service {
                id: 0xd220,
                label: Some("Dlf Kultur".into()),
                short_label: Some("Kultur".into()),
                components: vec![Component {
                    subchannel: 11,
                    ascty: 63,
                    primary: true,
                }],
                ..Default::default()
            },
        );
        e
    }

    #[test]
    fn a_scanned_ensemble_becomes_stations() {
        let list = stations(&ensemble(), "5C", 178_352_000);
        assert_eq!(
            list,
            vec![DabStation {
                service_id: 0xd220,
                label: "Dlf Kultur".into(),
                short_label: "Kultur".into(),
                ensemble: "DR Deutschland".into(),
                channel: "5C".into(),
                frequency_hz: 178_352_000,
                favorite: false,
                hidden: false,
                logo: None,
            }]
        );
        assert!(labels_complete(&ensemble()));
    }

    #[test]
    fn labels_are_incomplete_until_every_service_has_one() {
        let mut e = ensemble();
        e.services.get_mut(&0xd220).unwrap().label = None;
        assert!(!labels_complete(&e));
        assert!(!labels_complete(&Ensemble::default()));
    }

    #[test]
    fn logos_are_kept_per_station() {
        let dir = std::env::temp_dir().join(format!("livi-dab-logos-{}", std::process::id()));
        keep_image(&dir, 0xd220, "image/jpeg", b"jpg");
        assert_eq!(image_path(&dir, 0xd220), Some(dir.join("0000d220.jpg")));
        keep_image(&dir, 0xd220, "image/png", b"png");
        assert_eq!(image_path(&dir, 0xd220), Some(dir.join("0000d220.png")));
        assert!(!dir.join("0000d220.jpg").exists());
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let loaded = load_images(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&0xd220], "cG5n");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_images(&dir).is_empty());
    }
}
