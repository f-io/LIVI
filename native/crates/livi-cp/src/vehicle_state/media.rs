// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Radio {
    pub available: bool,
    pub dab: RadioSource,
    pub fm: RadioSource,
    pub band: RadioBand,
    pub frequency: Option<u32>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Vec<u8>,
    pub playing: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RadioSource {
    pub stations: Vec<RadioStation>,
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub ensemble: String,
    pub frequency: Option<u32>,
    pub image: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RadioBand {
    #[default]
    Dab,
    Fm,
}

impl RadioBand {
    pub(super) fn source(self) -> &'static str {
        match self {
            RadioBand::Dab => RADIO_SOURCE,
            RadioBand::Fm => FM_SOURCE,
        }
    }

    pub(super) fn named(source: &str) -> Option<Self> {
        [RadioBand::Dab, RadioBand::Fm]
            .into_iter()
            .find(|b| b.source() == source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioCommand {
    Tune(RadioBand, String),
    TuneFrequency(u32),
    Source(RadioBand),
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
pub(super) const RADIO_ITEMS_NOTIFIER: i64 = 4105;
pub(super) const RADIO_ITEM_IMAGES_NOTIFIER: i64 = 4106;
pub(super) const FM_SOURCE: &str = "FM";
pub(super) const FM_SEMANTIC: i64 = 2;
pub(super) const FM_SOURCE_SERVICE: i64 = 4112;
pub(super) const FM_DISABLED: i64 = 4115;
pub(super) const FM_CURRENT_ITEM: i64 = 4116;
pub(super) const FM_FREQUENCY: i64 = 4117;
pub(super) const FM_ITEMS: i64 = 4118;
pub(super) const FM_ITEMS_NOTIFIER: i64 = 4119;
pub(super) const FM_LOWEST_KHZ: u32 = 87_500;
pub(super) const FM_HIGHEST_KHZ: u32 = 108_000;
pub(super) const FM_STEP_KHZ: u32 = 100;
pub(super) const NOW_PLAYING_SERVICE: i64 = 4353;
pub(super) const NOW_PLAYING_SOURCE: i64 = 4354;
pub(super) const NOW_PLAYING_TITLE: i64 = 4355;
pub(super) const NOW_PLAYING_ARTIST: i64 = 4356;
pub(super) const NOW_PLAYING_ALBUM: i64 = 4357;
pub(super) const NOW_PLAYING_ARTWORK: i64 = 4358;
pub(super) const PLAYBACK_STATE: i64 = 4359;
pub(super) const NOW_PLAYING_ARTWORK_NOTIFIER: i64 = 4367;
pub(super) const PAUSED: i64 = 0;
pub(super) const PLAYING: i64 = 1;

pub(super) fn media() -> Accessory {
    Accessory {
        name: "media",
        iid: 4096,
        kind: 0x0300_0001,
        services: vec![dab_source(), fm_source()],
    }
}

fn source_head(service: i64, source: &str, semantic: i64) -> Vec<Characteristic> {
    use Format::*;
    vec![
        car(
            service + 1,
            0x3000_0019,
            "mediaSourceIdentifier",
            String,
            Value::str(source),
        )
        .fixed(),
        car(
            service + 2,
            0x3200_0025,
            "mediaSourceSemanticType",
            Uint8,
            Value::Int(semantic),
        )
        .up_to(8)
        .fixed(),
        car(
            service + 3,
            0x3200_0011,
            "mediaSourceDisabled",
            Bool,
            Value::Bool(true),
        ),
    ]
}

fn fm_source() -> Service {
    use Format::*;
    let mut characteristics = source_head(FM_SOURCE_SERVICE, FM_SOURCE, FM_SEMANTIC);
    characteristics.extend([
        car(
            FM_CURRENT_ITEM,
            0x3200_0026,
            "currentMediaItem",
            String,
            Value::str(""),
        )
        .invalidable(),
        car(
            FM_FREQUENCY,
            0x3200_0007,
            "currentFrequency",
            Uint32,
            Value::Int(i64::from(FM_LOWEST_KHZ)),
        )
        .between(
            f64::from(FM_LOWEST_KHZ),
            f64::from(FM_HIGHEST_KHZ),
            f64::from(FM_STEP_KHZ),
        )
        .invalidable(),
        car(
            FM_ITEMS,
            0x3200_0012,
            "mediaItems",
            Array,
            Value::Array(Vec::new()),
        )
        .large(FM_ITEMS_NOTIFIER),
    ]);
    Service {
        iid: FM_SOURCE_SERVICE,
        kind: 0x1400_0006,
        characteristics,
    }
}

fn dab_source() -> Service {
    use Format::*;
    let mut characteristics = source_head(RADIO_SOURCE_SERVICE, RADIO_SOURCE, DAB_SOURCE);
    characteristics.extend([
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
        )
        .invalidable(),
        car(
            RADIO_ITEMS,
            0x3200_0012,
            "mediaItems",
            Array,
            Value::Array(Vec::new()),
        )
        .large(RADIO_ITEMS_NOTIFIER),
        car(
            RADIO_ITEM_IMAGES,
            0x3200_0031,
            "mediaItemImages",
            Array,
            Value::Array(Vec::new()),
        )
        .large(RADIO_ITEM_IMAGES_NOTIFIER),
    ]);
    Service {
        iid: RADIO_SOURCE_SERVICE,
        kind: 0x1400_0006,
        characteristics,
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
                .large(NOW_PLAYING_ARTWORK_NOTIFIER),
                car(
                    PLAYBACK_STATE,
                    0x3200_0034,
                    "playbackState",
                    Uint8,
                    Value::Int(PAUSED),
                )
                .up_to(2),
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
                urgent: true,
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

pub(super) fn radio_item(s: &RadioStation, band: RadioBand) -> Value {
    let kind = match band {
        RadioBand::Dab => "dab",
        RadioBand::Fm => "fm",
    };
    let mut d = vec![
        ("identifier", Value::str(&s.id)),
        ("mediaItemName", Value::str(&s.name)),
        ("mediaItemShortName", Value::str(&s.short_name)),
        ("mediaItemType", Value::str(kind)),
        ("mediaItemCategory", Value::Int(0)),
    ];
    if !s.ensemble.is_empty() {
        d.push(("ensemble", Value::str(&s.ensemble)));
    }
    if let Some(khz) = s.frequency {
        d.push(("frequency", Value::Int(i64::from(khz))));
    }
    if s.image.is_some() {
        d.push(("mediaItemImageIdentifier", Value::str(&s.id)));
    }
    Value::Dict(d.into_iter().map(|(k, v)| (Value::str(k), v)).collect())
}
