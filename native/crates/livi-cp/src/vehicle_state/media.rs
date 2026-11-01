// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Radio {
    pub available: bool,
    pub stations: Vec<RadioStation>,
    pub current: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Vec<u8>,
    pub playing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub ensemble: String,
    pub image: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioCommand {
    Tune(String),
    Play,
    Pause,
    Next,
    Previous,
}

pub(super) const RADIO_SOURCE: &str = "DAB";
pub(super) const DAB_SOURCE: i64 = 6;
pub(super) const RADIO_SOURCE_SERVICE: i64 = 4097;
pub(super) const RADIO_DISABLED: i64 = 4100;
pub(super) const RADIO_CURRENT_ITEM: i64 = 4102;
pub(super) const RADIO_ITEMS: i64 = 4103;
pub(super) const RADIO_ITEM_IMAGES: i64 = 4104;
pub(super) const NOW_PLAYING_SERVICE: i64 = 4353;
pub(super) const NOW_PLAYING_SOURCE: i64 = 4354;
pub(super) const NOW_PLAYING_TITLE: i64 = 4355;
pub(super) const NOW_PLAYING_ARTIST: i64 = 4356;
pub(super) const NOW_PLAYING_ALBUM: i64 = 4357;
pub(super) const NOW_PLAYING_ARTWORK: i64 = 4358;
pub(super) const PLAYBACK_STATE: i64 = 4359;
pub(super) const PAUSED: i64 = 0;
pub(super) const PLAYING: i64 = 1;

pub(super) fn media() -> Accessory {
    use Format::*;
    Accessory {
        name: "media",
        iid: 4096,
        kind: 0x0300_0001,
        services: vec![Service {
            iid: RADIO_SOURCE_SERVICE,
            kind: 0x1400_0006,
            characteristics: vec![
                car(
                    4098,
                    0x3000_0019,
                    "mediaSourceIdentifier",
                    String,
                    Value::str(RADIO_SOURCE),
                )
                .fixed(),
                car(
                    4099,
                    0x3200_0025,
                    "mediaSourceSemanticType",
                    Uint8,
                    Value::Int(DAB_SOURCE),
                )
                .fixed(),
                car(
                    RADIO_DISABLED,
                    0x3200_0011,
                    "mediaSourceDisabled",
                    Bool,
                    Value::Bool(true),
                ),
                car(
                    4101,
                    0x3000_0001,
                    "mediaSourceLabel",
                    String,
                    Value::str("DAB+"),
                ),
                car(
                    RADIO_CURRENT_ITEM,
                    0x3200_0026,
                    "currentMediaItem",
                    String,
                    Value::str(""),
                ),
                car(
                    RADIO_ITEMS,
                    0x3200_0012,
                    "mediaItems",
                    Array,
                    Value::Array(Vec::new()),
                )
                .large(),
                car(
                    RADIO_ITEM_IMAGES,
                    0x3200_0031,
                    "mediaItemImages",
                    Array,
                    Value::Array(Vec::new()),
                )
                .large(),
            ],
        }],
    }
}

pub(super) fn now_playing() -> Accessory {
    use Format::*;
    Accessory {
        name: "nowPlaying",
        iid: 4352,
        kind: 0x0F00_0001,
        services: vec![Service {
            iid: NOW_PLAYING_SERVICE,
            kind: 0x1400_0001,
            characteristics: vec![
                car(
                    NOW_PLAYING_SOURCE,
                    0x3200_0023,
                    "currentMediaSource",
                    String,
                    Value::str(RADIO_SOURCE),
                ),
                car(
                    NOW_PLAYING_TITLE,
                    0x3200_0003,
                    "title",
                    String,
                    Value::str(""),
                ),
                car(
                    NOW_PLAYING_ARTIST,
                    0x3200_0004,
                    "artist",
                    String,
                    Value::str(""),
                ),
                car(
                    NOW_PLAYING_ALBUM,
                    0x3200_0005,
                    "album",
                    String,
                    Value::str(""),
                ),
                car(
                    NOW_PLAYING_ARTWORK,
                    0x3200_0020,
                    "artwork",
                    Data,
                    Value::Data(Vec::new()),
                )
                .large(),
                car(
                    PLAYBACK_STATE,
                    0x3200_0034,
                    "playbackState",
                    Uint8,
                    Value::Int(PAUSED),
                ),
            ],
        }],
    }
}

pub(super) fn radio_controls() -> Vec<(i64, Control)> {
    use Format::*;
    let command = |iid, kind, name, parameters| {
        (
            NOW_PLAYING_SERVICE,
            Control {
                iid,
                kind,
                name,
                parameters,
                response: Vec::new(),
            },
        )
    };
    vec![
        command(4360, 0x0F00_0032, "play", Vec::new()),
        command(4361, 0x0F00_0033, "pause", Vec::new()),
        command(4362, 0x0F00_0035, "nextItem", Vec::new()),
        command(4363, 0x0F00_0036, "previousItem", Vec::new()),
        command(
            4364,
            0x0F00_004B,
            "tuneToIdentifier",
            vec![("identifier", String), ("sourceIdentifier", String)],
        ),
        command(
            4365,
            0x0F00_004C,
            "tuneToFrequency",
            vec![("frequency", Uint32), ("sourceIdentifier", String)],
        ),
        command(
            4366,
            0x0F00_004D,
            "changeMediaSource",
            vec![("identifier", String)],
        ),
    ]
}

pub(super) fn radio_item(s: &RadioStation) -> Value {
    let mut d = vec![
        ("identifier", Value::str(&s.id)),
        ("mediaItemName", Value::str(&s.name)),
        ("mediaItemShortName", Value::str(&s.short_name)),
        ("mediaItemType", Value::str("dab")),
        ("ensemble", Value::str(&s.ensemble)),
        ("mediaItemCategory", Value::Int(0)),
    ];
    if s.image.is_some() {
        d.push(("mediaItemImageIdentifier", Value::str(&s.id)));
    }
    Value::Dict(d.into_iter().map(|(k, v)| (Value::str(k), v)).collect())
}
