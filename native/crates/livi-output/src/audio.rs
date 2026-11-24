// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;
use livi_output_proto::audio;

impl<O: Outside> Host<O> {
    pub(super) fn open_audio(&mut self, id: u32, rest: &[u8]) {
        let Some((open, key)) = audio::parse_audio_open(rest) else {
            eprintln!("livi: audio 0x{id:x} open is malformed");
            return;
        };
        let fed = open.fed;
        let cfg = AudioConfig {
            codec: open.codec,
            payload_type: open.payload_type,
            clock_rate: open.clock_rate,
            channels: open.channels,
            latency_ms: open.latency_ms,
            realtime: open.realtime,
            paced: open.paced,
            device: Some(open.device).filter(|d| !d.is_empty()),
            key,
            codec_data: if open.codec == audio::AudioCodec::AacRaw {
                audio::aac_raw_asc(&key).to_vec()
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
        if self.muted {
            speaker.set_volume(0.0, 0);
        }
        let speaker = Output::new(speaker);

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
                activated: 0,
                smoothed_ahead_ms: None,
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
            speaker.set_volume(if self.muted { 0.0 } else { level }, 0);
        }
        if self.visualizer_enabled {
            speaker.set_visualizer_enabled(true);
        }
        Some(speaker)
    }

    pub(super) fn open_uplink(&mut self, id: u32, rest: &[u8]) {
        let Some((open, key)) = audio::parse_mic_open(rest) else {
            eprintln!("livi: microphone 0x{id:x} open is malformed");
            return;
        };
        let cfg = UplinkConfig {
            codec: if open.pcm {
                UplinkCodec::Pcm
            } else {
                UplinkCodec::Opus
            },
            payload_type: open.payload_type,
            sample_rate: open.sample_rate,
            channels: open.channels,
            bitrate: open.bitrate,
            frame_ms: open.frame_ms,
            port: open.port,
            key,
            phone: open.phone,
            device: Some(open.device).filter(|d| !d.is_empty()),
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

    pub(super) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        for a in self.audio.borrow().values() {
            let level = if muted { 0.0 } else { a.level.unwrap_or(1.0) };
            a.speaker.get().set_volume(level, 0);
        }
        for (id, p) in &self.url_players {
            p.set_muted(muted || self.url_muted.contains(id));
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

    pub fn pump_audio_info(&self) {
        let mut streams = self.audio.borrow_mut();
        let heard = streams
            .iter_mut()
            .filter(|(_, a)| a.active.load(Ordering::Relaxed))
            .max_by_key(|(_, a)| a.activated);
        let info = heard.and_then(|(id, a)| {
            let speaker = a.speaker.get();
            let now = f64::from(speaker.ahead_ms()?);
            let smoothed = a
                .smoothed_ahead_ms
                .map_or(now, |s| s + DELAY_WEIGHT * (now - s));
            a.smoothed_ahead_ms = Some(smoothed);
            let (rate, channels) = speaker
                .decoded()
                .unwrap_or((a.cfg.clock_rate, a.cfg.channels));
            let info = audio::AudioInfo {
                codec: a.cfg.codec,
                realtime: a.cfg.realtime,
                rate,
                channels,
                ahead_ms: smoothed.round() as u32,
                target_ms: speaker.target_ms(),
            };
            Some((*id, info))
        });
        let mut told = self.told_info.borrow_mut();
        let same = match (&*told, &info) {
            (None, None) => true,
            (Some((was_id, was)), Some((id, now))) => {
                was_id == id
                    && audio::AudioInfo {
                        ahead_ms: was.ahead_ms,
                        ..*now
                    } == *was
                    && was.ahead_ms.abs_diff(now.ahead_ms) < DELAY_STEP_MS
            }
            _ => false,
        };
        if same {
            return;
        }
        match &info {
            Some((id, i)) => self
                .wire
                .reply(REPLY_AUDIO_INFO, *id, &audio::encode_audio_info(i)),
            None => self.wire.reply(REPLY_AUDIO_INFO, 0, &[]),
        }
        *told = info;
    }

    pub fn pump_url_status(&self) {
        for (id, player) in &self.url_players {
            self.wire
                .reply(REPLY_URL_STATUS, *id, &url_status_body(&player.status()));
        }
    }
}
