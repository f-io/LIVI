// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn level(&self, kind: AudioKind) -> f64 {
        let duck = if kind == AudioKind::Media {
            self.duck_level
        } else {
            1.0
        };
        self.levels.get(kind) * duck
    }

    pub(super) fn push_level(&self, kind: AudioKind, ramp_ms: u32) {
        self.to_active(Do::Volume {
            kind,
            level: self.level(kind),
            ramp_ms,
        });
    }

    pub(super) fn push_all_levels(&self) {
        for kind in [
            AudioKind::Media,
            AudioKind::Alert,
            AudioKind::Speech,
            AudioKind::Call,
        ] {
            self.push_level(kind, 0);
        }
    }

    pub(super) fn duck(&mut self, level: f64, ramp_ms: u32) {
        self.duck_level = level.clamp(0.0, 1.0);
        self.duck_ramp_ms = ramp_ms;
        self.push_level(AudioKind::Media, ramp_ms);
    }

    pub(super) fn stream_volumes_to_phone(&self) {
        let levels = serde_json::json!({
            "audioVolume": self.levels.music,
            "navVolume": self.levels.nav,
            "callVolume": self.levels.call,
            "voiceAssistantVolume": self.levels.voice,
        });
        if let serde_json::Value::Object(levels) = levels {
            self.cp.send(CpCmd::Gauges(levels));
        }
    }

    pub(super) fn on_duck(&mut self, link: Link, level: f64, duration_ms: u32) {
        let Some(s) = self.by_link(link) else { return };
        s.duck_level = level;
        s.duck_ramp_ms = duration_ms;
        if self.is_active(link) {
            self.duck(if level >= 1.0 { 1.0 } else { level }, duration_ms);
        }
    }

    pub(super) fn call_audio(&mut self, up: bool) {
        if self.hands_free.call == up {
            return;
        }
        self.hands_free.call = up;
        self.activity(|a| a.phone = up);
        let _ = self.phone_io.send(Io::Call(up));
        self.attend(Attention::Call, up);
    }
}
