// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<O: Outside> Host<O> {
    pub(super) fn open_audio(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 44;
        const KEY_AT: usize = 12;
        if rest.len() < FIXED {
            return;
        }

        let cfg = AudioConfig {
            codec: match rest[0] {
                1 => AudioCodec::Opus,
                2 => AudioCodec::Lpcm,
                3 => AudioCodec::PcmLe,
                4 => AudioCodec::AacRaw,
                _ => AudioCodec::AacLc,
            },
            payload_type: rest[1],
            clock_rate: u32::from_le_bytes(rest[2..6].try_into().unwrap()),
            channels: rest[6],
            latency_ms: u32::from_le_bytes(rest[7..11].try_into().unwrap()),
            realtime: rest[11] & 1 != 0,
            device: match core::str::from_utf8(&rest[FIXED..]) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
            key: rest[KEY_AT..KEY_AT + 32].try_into().unwrap(),
            codec_data: if rest[0] == 4 {
                let len = (rest[KEY_AT] as usize).min(31);
                rest[KEY_AT + 1..KEY_AT + 1 + len].to_vec()
            } else {
                Vec::new()
            },
        };

        let Some(speaker) = self.outside.create_speaker(&cfg) else {
            eprintln!("livi: create audio 0x{id:x} FAILED");
            return;
        };
        if self.visualizer_enabled {
            speaker.set_visualizer_enabled(true);
        }
        let speaker = Output::new(speaker);

        let fed = rest[11] & 2 != 0;
        let active = Arc::new(AtomicBool::new(false));
        let feed = AudioFeed::<O> {
            speaker: speaker.clone(),
            wire: self.wire.clone(),
            id,
            active: active.clone(),
        };
        let (ears, data_port, control_port) = if fed {
            (None, 0, 0)
        } else {
            match self.outside.listen_audio(cfg.key, Box::new(feed)) {
                Some((ears, data, control)) => (Some(ears), data, control),
                None => return,
            }
        };

        self.audio.borrow_mut().insert(
            id,
            AudioStream {
                speaker,
                cfg,
                level: None,
                active,
                told_delay_ms: None,
                smoothed_delay_ms: None,
                _ears: ears,
            },
        );
        let mut ports = data_port.to_le_bytes().to_vec();
        ports.extend_from_slice(&control_port.to_le_bytes());
        self.wire.reply(REPLY_AUDIO_PORTS, id, &ports);
    }

    pub(super) fn set_audio_output(&mut self, rest: &[u8]) {
        let device = match core::str::from_utf8(rest) {
            Ok("") | Err(_) => None,
            Ok(name) => Some(name.to_owned()),
        };
        for (id, a) in self.audio.borrow_mut().iter_mut() {
            if a.cfg.device == device {
                continue;
            }
            let before = std::mem::replace(&mut a.cfg.device, device.clone());
            let Some(speaker) = self.fresh_speaker(&a.cfg, a.level) else {
                eprintln!("livi: audio 0x{id:x} cannot move to {device:?}, it stays");
                a.cfg.device = before;
                continue;
            };
            a.speaker.replace(speaker);
        }
    }

    pub(super) fn fresh_speaker(
        &self,
        cfg: &AudioConfig,
        level: Option<f64>,
    ) -> Option<O::Speaker> {
        let speaker = self.outside.create_speaker(cfg)?;
        if let Some(level) = level {
            speaker.set_volume(level, 0);
        }
        if self.visualizer_enabled {
            speaker.set_visualizer_enabled(true);
        }
        Some(speaker)
    }

    pub(super) fn open_uplink(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 50;
        const KEY_AT: usize = 17;
        if rest.len() < FIXED {
            return;
        }
        let phone_len = usize::from(rest[FIXED - 1]);
        if rest.len() < FIXED + phone_len {
            return;
        }
        let (phone, device) = rest[FIXED..].split_at(phone_len);

        let cfg = UplinkConfig {
            codec: if rest[0] == 1 {
                UplinkCodec::Pcm
            } else {
                UplinkCodec::Opus
            },
            payload_type: rest[1],
            sample_rate: u32::from_le_bytes(rest[2..6].try_into().unwrap()),
            channels: rest[6],
            bitrate: u32::from_le_bytes(rest[7..11].try_into().unwrap()),
            frame_ms: u32::from_le_bytes(rest[11..15].try_into().unwrap()),
            port: u16::from_le_bytes(rest[15..17].try_into().unwrap()),
            key: rest[KEY_AT..KEY_AT + 32].try_into().unwrap(),
            phone: String::from_utf8_lossy(phone).into_owned(),
            device: match core::str::from_utf8(device) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
        };

        match self.outside.open_uplink(cfg) {
            Some(uplink) => {
                self.uplinks.insert(id, uplink);
            }
            None => eprintln!("livi: open microphone 0x{id:x} FAILED"),
        }
    }
    pub(super) fn open_tap(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 6;
        if rest.len() < FIXED {
            return;
        }
        let device_len = usize::from(rest[FIXED - 1]);
        if rest.len() < FIXED + device_len {
            return;
        }
        let (device, path) = rest[FIXED..].split_at(device_len);
        let Ok(path) = core::str::from_utf8(path) else {
            return;
        };
        let cfg = TapConfig {
            sample_rate: u32::from_le_bytes(rest[0..4].try_into().unwrap()),
            channels: rest[4],
            device: match core::str::from_utf8(device) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
            path: path.to_owned(),
        };
        match self.outside.open_tap(cfg) {
            Some(tap) => {
                self.taps.insert(id, tap);
            }
            None => eprintln!("livi: open microphone tap 0x{id:x} FAILED"),
        }
    }

    pub(super) fn set_visualizer_enabled(&mut self, on: bool) {
        self.visualizer_enabled = on;
        self.spectrum.clear();
        for a in self.audio.borrow().values() {
            a.speaker.get().set_visualizer_enabled(on);
        }
    }

    pub fn pump_visualizer(&mut self) {
        if !self.visualizer_enabled {
            return;
        }
        for (id, a) in self.audio.borrow().iter() {
            if let Some((samples, rate)) = a.speaker.get().take_visualizer() {
                self.spectrum.push(*id, rate, &samples);
            }
        }
        if let Some(bands) = self.spectrum.pump() {
            let payload: Vec<u8> = bands.iter().flat_map(|b| b.to_le_bytes()).collect();
            self.wire.reply(REPLY_VISUALIZER, 0, &payload);
        }
    }

    pub fn pump_audio_delays(&self) {
        for (id, a) in self.audio.borrow_mut().iter_mut() {
            let Some(now) = a.speaker.get().output_delay_ms() else {
                continue;
            };
            let smoothed = a
                .smoothed_delay_ms
                .map_or(f64::from(now), |s| s + DELAY_WEIGHT * (f64::from(now) - s));
            a.smoothed_delay_ms = Some(smoothed);
            let ms = smoothed.round() as u32;
            if a.told_delay_ms
                .is_some_and(|told| told.abs_diff(ms) < DELAY_STEP_MS)
            {
                continue;
            }
            a.told_delay_ms = Some(ms);
            self.wire.reply(REPLY_AUDIO_DELAY, *id, &ms.to_le_bytes());
        }
    }

    pub fn pump_url_status(&self) {
        for (id, player) in &self.url_players {
            self.wire
                .reply(REPLY_URL_STATUS, *id, &url_status_body(&player.status()));
        }
    }
}
