// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) const VOLUME: i64 = 2306;
pub(super) const MUTE: i64 = 2309;
pub(super) const NAV_VOLUME: i64 = 2311;
pub(super) const CALL_VOLUME: i64 = 2314;
pub(super) const MUSIC_VOLUME: i64 = 2317;
pub(super) const SIRI_VOLUME: i64 = 2321;

pub(super) const LAST_VOLUME_TYPE: u32 = 7;
pub const VOLUMES: [(&str, i64); 5] = [
    ("volume", VOLUME),
    ("navVolume", NAV_VOLUME),
    ("callVolume", CALL_VOLUME),
    ("audioVolume", MUSIC_VOLUME),
    ("voiceAssistantVolume", SIRI_VOLUME),
];
pub(super) const VOLUME_TYPES: [(i64, i64, Option<&str>); 5] = [
    (VOLUME, 0, Some("LIVI")),
    (NAV_VOLUME, 1, None),
    (CALL_VOLUME, 2, None),
    (SIRI_VOLUME, 3, None),
    (MUSIC_VOLUME, 6, Some("Musik")),
];

pub(super) fn audio() -> Accessory {
    use Format::*;
    let volume = |iid: i64, kind: i64, label: Option<&str>| {
        let mut characteristics = vec![
            shared(iid, 0x3300_0006, "volume", Uint8, Value::Int(50))
                .up_to(100)
                .enabled(),
            car(iid + 1, 0x3300_0005, "volumeType", Uint8, Value::Int(kind))
                .up_to(LAST_VOLUME_TYPE)
                .fixed(),
        ];
        if let Some(label) = label {
            characteristics.push(
                car(
                    iid + 2,
                    0x3000_0001,
                    "volumeLabel",
                    String,
                    Value::str(label),
                )
                .fixed(),
            );
        }
        if iid == VOLUME {
            characteristics.push(shared(MUTE, 0x3300_0009, "mute", Bool, Value::Bool(false)));
        }
        Service {
            iid: iid - 1,
            kind: 0x1300_0002,
            characteristics,
        }
    };
    Accessory {
        name: "audio",
        iid: 2304,
        kind: 0x0200_0001,
        services: VOLUME_TYPES
            .iter()
            .map(|(iid, kind, label)| volume(*iid, *kind, *label))
            .collect(),
    }
}
