// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_audio::player::uplink::{SocketTap, Uplink, UplinkConfig as UplinkPipeline};
use livi_audio::player::{Config as AudioPipeline, Player as AudioPipelinePlayer};
use livi_audio::stream::AudioSink;
use livi_screen_stream::ScreenSink;
#[cfg(not(target_os = "macos"))]
use livi_video_player::{Player, UrlPlayer};

#[cfg(target_os = "macos")]
use crate::ui_plane::{UiPlane, UiUrlPlayer};
use crate::{AudioConfig, MediaSink, Outside, Speaker, TapConfig, UplinkConfig};
#[cfg(not(target_os = "macos"))]
use crate::{Plane, UrlPlay};

pub struct Tap(#[allow(dead_code)] SocketTap);

#[cfg(not(target_os = "macos"))]
impl Plane for Player {
    fn start(&self) {
        Player::start(self)
    }

    fn push(&self, nal: &[u8]) {
        Player::push(self, nal);
    }

    fn flush(&self) {
        Player::flush(self)
    }

    fn set_gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
        Player::set_gamma(self, gamma, contrast, r, g, b)
    }
}

#[cfg(not(target_os = "macos"))]
impl UrlPlay for UrlPlayer {
    fn start(&self) {
        UrlPlayer::start(self)
    }

    fn set_rate(&self, rate: f64) {
        UrlPlayer::set_rate(self, rate)
    }

    fn seek(&self, seconds: f64) {
        UrlPlayer::seek(self, seconds)
    }

    fn set_muted(&self, muted: bool) {
        UrlPlayer::set_muted(self, muted)
    }

    fn status(&self) -> crate::UrlStatus {
        UrlPlayer::status(self)
    }
}

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
    #[cfg(not(target_os = "macos"))]
    type Plane = Player;
    #[cfg(target_os = "macos")]
    type Plane = UiPlane;
    #[cfg(not(target_os = "macos"))]
    type UrlPlayer = UrlPlayer;
    #[cfg(target_os = "macos")]
    type UrlPlayer = UiUrlPlayer;
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

    #[cfg(not(target_os = "macos"))]
    fn create_plane(&self, _id: u32, codec: &str, codec_data: &[u8]) -> Option<Player> {
        Player::new(codec, 0, codec_data)
    }

    #[cfg(target_os = "macos")]
    fn create_plane(&self, id: u32, codec: &str, codec_data: &[u8]) -> Option<UiPlane> {
        UiPlane::open(&ui_planes_path(id)?, id, codec, codec_data)
    }

    #[cfg(not(target_os = "macos"))]
    type Camera = livi_video_player::CameraPlayer;
    #[cfg(target_os = "macos")]
    type Camera = crate::ui_plane::UiCamera;

    #[cfg(not(target_os = "macos"))]
    fn open_camera(
        &self,
        _id: u32,
        order: &crate::CameraOrder,
    ) -> Option<livi_video_player::CameraPlayer> {
        let camera =
            livi_video_player::CameraPlayer::new(order, 0, livi_video_player::to_stderr())?;
        camera.start();
        Some(camera)
    }

    #[cfg(target_os = "macos")]
    fn open_camera(
        &self,
        id: u32,
        order: &crate::CameraOrder,
    ) -> Option<crate::ui_plane::UiCamera> {
        crate::ui_plane::UiCamera::open(&ui_planes_path(id)?, id, order)
    }

    fn watch_cameras(&self, report: Box<dyn Fn(Vec<crate::CameraSeen>) + Send>) {
        livi_video_player::watch_cameras(report);
    }

    #[cfg(not(target_os = "macos"))]
    fn play_url(&self, _id: u32, url: &str, audio_device: &str) -> Option<UrlPlayer> {
        UrlPlayer::new(url, audio_device, 0, livi_video_player::to_stderr())
    }

    #[cfg(target_os = "macos")]
    fn play_url(&self, id: u32, url: &str, audio_device: &str) -> Option<UiUrlPlayer> {
        UiUrlPlayer::open(&ui_planes_path(id)?, id, url, audio_device)
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

pub fn probe_json() -> String {
    livi_video_player::ensure_init();
    let mut out = String::from("{");
    for (i, codec) in ["h264", "h265", "vp9", "av1"].iter().enumerate() {
        let (hw, sw) = livi_video_player::probe(codec);
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{codec}\":{{\"hw\":{hw},\"sw\":{sw}}}"));
    }
    out.push('}');
    out
}

#[cfg(target_os = "macos")]
fn ui_planes_path(id: u32) -> Option<String> {
    std::env::var(livi_host_proto::ui_planes::PATH_ENV)
        .inspect_err(|_| eprintln!("[gst-host] no UI to draw plane 0x{id:x} in"))
        .ok()
}
