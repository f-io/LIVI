// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::Protocol;
use livi_cp::stack::AudioKind;
use serde::Serialize;

use crate::radio::Band;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    pub media: bool,
    pub speech: bool,
    pub system: bool,
    pub phone: bool,
    pub voice_assistant: bool,
    pub nav_announcing: bool,
}

impl Activity {
    pub fn stream(&mut self, kind: AudioKind, playing: bool) {
        match kind {
            AudioKind::Media => self.media = playing,
            AudioKind::Call => self.phone = playing,
            AudioKind::Speech => self.voice_assistant = playing,
            AudioKind::Alert => {
                self.nav_announcing = playing;
                self.speech = playing;
            }
        }
    }
}

#[derive(Serialize)]
struct Playing {
    playing: bool,
}

#[derive(Serialize)]
struct Active {
    active: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Projection {
    active: Option<&'static str>,
    streaming: bool,
    phone_type: Option<&'static str>,
}

#[derive(Serialize)]
struct Audio {
    media: Playing,
    speech: Playing,
    system: Playing,
}

#[derive(Serialize)]
struct Nav {
    announcing: bool,
}

#[derive(Serialize)]
struct Media {
    source: &'static str,
}

#[derive(Serialize)]
struct Ui {
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    ui: Ui,
    projection: Projection,
    audio: Audio,
    phone: Active,
    voice_assistant: Active,
    nav: Nav,
    media: Media,
}

const DAB_SOURCE: &str = "dabplus";
const FM_SOURCE: &str = "fm";

pub fn band(source: &str) -> Option<Band> {
    match source {
        DAB_SOURCE => Some(Band::Dab),
        FM_SOURCE => Some(Band::Fm),
        _ => None,
    }
}

pub fn payload(path: &str, active: Option<Protocol>, a: Activity, radio: Option<Band>) -> Payload {
    let (protocol, phone_type) = match active {
        Some(Protocol::Carplay) => (Some("cp"), Some("CarPlay")),
        Some(Protocol::Androidauto) => (Some("aa"), Some("AndroidAuto")),
        Some(Protocol::Airplay) => (Some("ap"), Some("AirPlay")),
        None => (None, None),
    };
    Payload {
        ui: Ui {
            path: path.to_string(),
        },
        projection: Projection {
            active: protocol,
            streaming: active.is_some(),
            phone_type,
        },
        audio: Audio {
            media: Playing { playing: a.media },
            speech: Playing { playing: a.speech },
            system: Playing { playing: a.system },
        },
        phone: Active { active: a.phone },
        voice_assistant: Active {
            active: a.voice_assistant,
        },
        nav: Nav {
            announcing: a.nav_announcing,
        },
        media: Media {
            source: match radio {
                Some(Band::Dab) => DAB_SOURCE,
                Some(Band::Fm) => FM_SOURCE,
                None => "phone",
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn the_payload_keeps_its_old_shape() {
        let mut a = Activity::default();
        a.stream(AudioKind::Alert, true);
        a.stream(AudioKind::Call, true);
        let text = serde_json::to_string_pretty(&payload(
            "/media",
            Some(Protocol::Carplay),
            a,
            Some(Band::Dab),
        ))
        .unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v,
            json!({
                "ui": { "path": "/media" },
                "projection": { "active": "cp", "streaming": true, "phoneType": "CarPlay" },
                "audio": {
                    "media": { "playing": false },
                    "speech": { "playing": true },
                    "system": { "playing": false }
                },
                "phone": { "active": true },
                "voiceAssistant": { "active": false },
                "nav": { "announcing": true },
                "media": { "source": "dabplus" }
            })
        );
        let keys: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("  \"") && l.ends_with('{'))
            .map(|l| l.trim().split('"').nth(1).unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "ui",
                "projection",
                "audio",
                "phone",
                "voiceAssistant",
                "nav",
                "media"
            ]
        );
    }

    #[test]
    fn android_auto_reads_as_aa() {
        let aa = payload("", Some(Protocol::Androidauto), Activity::default(), None);
        assert_eq!(
            (aa.projection.active, aa.projection.phone_type),
            (Some("aa"), Some("AndroidAuto"))
        );
    }

    #[test]
    fn the_radio_band_is_the_media_source() {
        for b in [Band::Dab, Band::Fm] {
            let p = payload("", None, Activity::default(), Some(b));
            assert_eq!(band(p.media.source), Some(b));
        }
        assert_eq!(
            payload("", None, Activity::default(), None).media.source,
            "phone"
        );
        assert_eq!(band("phone"), None);
    }
}
