// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::future::Future;

use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    AacLc,
    Opus,
    Pcm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRequest {
    pub codec: AudioCodec,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub channels: u8,
    pub latency_ms: u32,
    pub realtime: bool,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicRequest {
    pub opus: bool,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub frame_ms: u32,
    pub port: u16,
    pub phone: String,
    pub device: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStream {
    pub id: u32,
    pub data_port: u16,
    pub control_port: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VideoStatus {
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub seekable: Option<(f64, f64)>,
    pub playing: bool,
    pub ready: bool,
    pub buffered_percent: Option<u8>,
    pub ended: bool,
    pub failed: bool,
}

pub trait Media: Send + Sync + 'static {
    fn open_screen(&self, cluster: bool, key: [u8; 32]) -> impl Future<Output = (u16, u32)> + Send;
    fn set_screen_active(&self, receiver: u32, active: bool);
    fn close_screen(&self, receiver: u32);
    fn open_audio(
        &self,
        key: [u8; 32],
        req: AudioRequest,
    ) -> impl Future<Output = AudioStream> + Send;
    fn set_audio_active(&self, stream: u32, active: bool);
    fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32);
    fn audio_output_delay(&self, stream: u32) -> Option<u32>;
    fn close_audio(&self, stream: u32);
    fn open_mic(&self, key: [u8; 32], req: MicRequest) -> u32;
    fn close_mic(&self, id: u32);
    fn audio_started(&self) -> broadcast::Receiver<(u32, u32)>;
    fn play_video(&self, url: String, audio_device: String, muted: bool);
    fn set_video_rate(&self, rate: f64);
    fn seek_video(&self, seconds: f64);
    fn set_video_muted(&self, muted: bool);
    fn stop_video(&self);
    fn set_video_shown(&self, shown: bool);
    fn video_status(&self) -> Option<VideoStatus>;
}
