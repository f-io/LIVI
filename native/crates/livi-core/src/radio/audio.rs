// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::State;
use livi_media::gst_host::{AudioCodec, AudioOpts, GstHost};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub codec: AudioCodec,
    pub key: [u8; 32],
    pub rate: u32,
    pub channels: u8,
}

#[derive(Default)]
pub struct Audio {
    pub stream: Option<u32>,
    level: f64,
}

impl Audio {
    pub async fn open(&mut self, gst: &GstHost, s: &State, f: Format) {
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
        gst.set_audio_volume(id, s.config.audio_volume, 0);
        self.stream = Some(id);
        self.level = s.config.audio_volume;
    }

    pub fn push(&self, gst: &GstHost, units: &[Vec<u8>]) {
        if let Some(id) = self.stream {
            for u in units {
                gst.push_audio(id, u);
            }
        }
    }

    pub fn follow(&mut self, gst: &GstHost, s: &State) {
        let Some(id) = self.stream else { return };
        if s.config.disable_audio_output {
            self.close(gst);
            return;
        }
        if s.config.audio_volume != self.level {
            self.level = s.config.audio_volume;
            gst.set_audio_volume(id, self.level, 100);
        }
    }

    pub fn close(&mut self, gst: &GstHost) {
        if let Some(id) = self.stream.take() {
            gst.close_audio(id);
        }
    }
}
