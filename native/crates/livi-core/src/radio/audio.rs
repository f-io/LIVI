// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::State;
use livi_media::output::{AudioCodec, AudioOpts, Output};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub codec: AudioCodec,
    pub key: [u8; 32],
    pub rate: u32,
    pub channels: u8,
}

pub struct Audio {
    pub stream: Option<u32>,
    level: f64,
    duck: f64,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            stream: None,
            level: 0.0,
            duck: 1.0,
        }
    }
}

impl Audio {
    pub async fn open(&mut self, gst: &Output, s: &State, f: Format) {
        self.close(gst);
        if s.config.disable_audio_output {
            return;
        }
        let device = s.config.audio_output_device.clone().unwrap_or_default();
        let opts = AudioOpts {
            codec: f.codec,
            payload_type: 0,
            clock_rate: f.rate,
            channels: f.channels,
            latency_ms: 0,
            realtime: false,
            fed: true,
            device,
        };
        let (id, _, _) = gst.open_audio(&f.key, &opts).await;
        if id == 0 {
            return;
        }
        gst.set_audio_active(id, true);
        self.stream = Some(id);
        self.level = s.config.audio_volume;
        gst.set_audio_volume(id, self.level * self.duck, 0);
    }

    pub fn push(&self, gst: &Output, units: &[Vec<u8>]) {
        if let Some(id) = self.stream {
            for u in units {
                gst.push_audio(id, u);
            }
        }
    }

    pub fn follow(&mut self, gst: &Output, s: &State) {
        let Some(id) = self.stream else { return };
        if s.config.disable_audio_output {
            self.close(gst);
            return;
        }
        if s.config.audio_volume != self.level {
            self.level = s.config.audio_volume;
            gst.set_audio_volume(id, self.level * self.duck, 100);
        }
    }

    pub fn duck(&mut self, gst: &Output, level: f64, ramp_ms: u32) {
        self.duck = level.clamp(0.0, 1.0);
        if let Some(id) = self.stream {
            gst.set_audio_volume(id, self.level * self.duck, ramp_ms);
        }
    }

    pub fn close(&mut self, gst: &Output) {
        if let Some(id) = self.stream.take() {
            gst.close_audio(id);
        }
    }
}
