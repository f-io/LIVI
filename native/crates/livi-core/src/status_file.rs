// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::Protocol;
use livi_cp::stack::AudioKind;
use serde::Serialize;

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

pub fn payload(path: &str, active: Option<Protocol>, a: Activity, dab: bool) -> Payload {
    let (protocol, phone_type) = match active {
        Some(Protocol::Carplay) => (Some("cp"), Some("CarPlay")),
        Some(Protocol::Androidauto) => (Some("aa"), Some("AndroidAuto")),
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
            source: if dab { "dabplus" } else { "phone" },
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
        let text =
            serde_json::to_string_pretty(&payload("/media", Some(Protocol::Carplay), a, true))
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
        let aa = payload("", Some(Protocol::Androidauto), Activity::default(), false);
        assert_eq!(
            (aa.projection.active, aa.projection.phone_type),
            (Some("aa"), Some("AndroidAuto"))
        );
    }
}
