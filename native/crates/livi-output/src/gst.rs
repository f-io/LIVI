// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_audio::player::uplink::{SocketTap, Uplink, UplinkConfig as UplinkPipeline};
use livi_audio::player::{Config as AudioPipeline, Player as AudioPipelinePlayer};
use livi_audio::stream::AudioSink;
use livi_output_proto::{Decoders, Probe};
use livi_screen_stream::ScreenSink;

use crate::{AudioConfig, MediaSink, Outside, Speaker, TapConfig, UplinkConfig, VideoCodec};

#[cfg_attr(target_os = "android", path = "gst/android.rs")]
#[cfg_attr(target_os = "macos", path = "gst/mac.rs")]
#[cfg_attr(
    not(any(target_os = "macos", target_os = "android")),
    path = "gst/linux.rs"
)]
mod platform;
#[cfg(not(target_os = "macos"))]
mod players;

pub struct Tap(#[allow(dead_code)] SocketTap);

pub struct Ears(#[allow(dead_code)] livi_screen_stream::receiver::ScreenReceiver);

pub struct AudioEars(#[allow(dead_code)] livi_audio::stream::receiver::AudioReceiver);

pub struct FeedEars(#[allow(dead_code)] crate::feed::FeedListener);

impl Speaker for AudioPipelinePlayer {
    fn push_rtp(&self, rtp: &[u8]) {
        AudioPipelinePlayer::push_rtp(self, rtp);
    }

    fn push_samples(&self, samples: &[u8]) {
        AudioPipelinePlayer::push_samples(self, samples);
    }

    fn set_volume(&self, level: f64, ms: u64) {
        AudioPipelinePlayer::set_volume(self, level, ms)
    }

    fn set_visualizer_enabled(&self, on: bool) {
        AudioPipelinePlayer::set_visualizer_enabled(self, on)
    }

    fn take_visualizer(&self) -> Option<(Vec<u8>, u32)> {
        AudioPipelinePlayer::take_visualizer(self)
    }

    fn output_delay_ms(&self) -> Option<u32> {
        AudioPipelinePlayer::output_delay(self).and_then(|d| u32::try_from(d.as_millis()).ok())
    }
}

pub struct Gst;

impl Outside for Gst {
    type Plane = platform::Plane;
    type UrlPlayer = platform::Url;
    type Camera = platform::Camera;
    type Ears = Ears;
    type Speaker = AudioPipelinePlayer;
    type AudioEars = AudioEars;
    type Uplink = Uplink;
    type Tap = Tap;

    fn open_tap(&self, cfg: TapConfig) -> Option<Tap> {
        SocketTap::open(
            &cfg.path,
            cfg.sample_rate,
            cfg.channels,
            cfg.device.as_deref(),
            "tap",
        )
        .map(Tap)
    }

    fn create_plane(
        &self,
        id: u32,
        codec: VideoCodec,
        codec_data: &[u8],
    ) -> Option<platform::Plane> {
        platform::create_plane(id, codec, codec_data)
    }

    fn open_camera(&self, id: u32, order: &crate::CameraOrder) -> Option<platform::Camera> {
        platform::open_camera(id, order)
    }

    fn watch_cameras(&self, report: Box<dyn Fn(Vec<crate::CameraSeen>) + Send>) {
        livi_video_player::watch_cameras(report);
    }

    fn play_url(&self, id: u32, url: &str, audio_device: &str) -> Option<platform::Url> {
        platform::play_url(id, url, audio_device)
    }

    fn listen(&self, key: [u8; 32], sink: Box<dyn ScreenSink>) -> Option<(Ears, u16)> {
        match livi_screen_stream::receiver::ScreenReceiver::new(key, sink) {
            Ok((r, port)) => Some((Ears(r), port)),
            Err(e) => {
                eprintln!("[cp_screen] cannot listen: {e}");
                None
            }
        }
    }

    fn open_uplink(&self, cfg: UplinkConfig) -> Option<Uplink> {
        let uplink = Uplink::new(UplinkPipeline {
            codec: cfg.codec,
            payload_type: cfg.payload_type,
            sample_rate: cfg.sample_rate,
            channels: cfg.channels,
            bitrate: cfg.bitrate,
            frame_ms: cfg.frame_ms,
            key: cfg.key,
            device: cfg.device,
            phone: cfg.phone,
            port: cfg.port,
            label: String::from("mic"),
        })?;
        uplink.start();
        Some(uplink)
    }

    fn create_speaker(&self, cfg: &AudioConfig) -> Option<AudioPipelinePlayer> {
        let player = AudioPipelinePlayer::new(&AudioPipeline {
            codec: cfg.codec,
            payload_type: cfg.payload_type,
            clock_rate: cfg.clock_rate,
            channels: cfg.channels,
            latency_ms: cfg.latency_ms,
            realtime: cfg.realtime,
            device: cfg.device.clone(),
            label: format!("{:?}", cfg.codec),
            codec_data: cfg.codec_data.clone(),
        })?;
        player.start();
        Some(player)
    }

    fn listen_audio(
        &self,
        key: [u8; 32],
        sink: Box<dyn AudioSink + Send>,
    ) -> Option<(AudioEars, u16, u16)> {
        let stream = livi_audio::stream::AudioStream::new(key, sink);
        match livi_audio::stream::receiver::AudioReceiver::new(stream) {
            Ok((r, data, control)) => Some((AudioEars(r), data, control)),
            Err(e) => {
                eprintln!("[cp_audio] cannot listen: {e}");
                None
            }
        }
    }

    type FeedEars = FeedEars;

    fn open_feed(&self, path: &str, sink: Box<dyn MediaSink>) -> Option<FeedEars> {
        match crate::feed::FeedListener::new(path, sink) {
            Ok(l) => Some(FeedEars(l)),
            Err(e) => {
                eprintln!("[feed] cannot listen on {path}: {e}");
                None
            }
        }
    }
}

pub fn probe() -> Probe {
    livi_video_player::ensure_init();
    Probe::of(|codec| {
        let (hw, sw) = livi_video_player::probe(codec);
        Decoders { hw, sw }
    })
}
