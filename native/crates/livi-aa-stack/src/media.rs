// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::future::Future;

use tokio::sync::broadcast;

use crate::channels::audio::AudioChannelType;
use crate::discovery::VideoCodec;
use crate::manager::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioKind {
    Speech = 1,
    Call = 2,
    Media = 3,
    Alert = 4,
}

impl AudioKind {
    pub fn of(channel: AudioChannelType) -> Self {
        match channel {
            AudioChannelType::Media => Self::Media,
            AudioChannelType::Speech | AudioChannelType::System => Self::Alert,
            AudioChannelType::Telephony => Self::Call,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutput {
    pub kind: AudioKind,
    pub stream: u32,
    pub tag: Option<String>,
}

pub trait AaMedia: Send + Sync + 'static {
    fn feed_path(&self) -> impl Future<Output = String> + Send;
    fn video_feed(&self, session: SessionId, cluster: bool) -> u32;
    fn prime_video(&self, session: SessionId, cluster: bool, codec: VideoCodec);
    fn video_started(&self, cluster: bool, width: u32, height: u32);
    fn audio_feed(&self, session: SessionId, stream: u32) -> u32;
    fn set_active(&self, session: SessionId, active: bool);
    fn release(&self, session: SessionId);
    fn audio_outputs(&self) -> Vec<AudioOutput>;
    fn audio_output_opened(&self) -> broadcast::Receiver<AudioOutput>;
    fn prime_audio(&self, kind: AudioKind, sample_rate: u32, channels: u32, tag: &str);
    fn set_host_volume(&self, kind: AudioKind, level: f64, ramp_ms: u32);
    fn open_mic_tap(
        &self,
        path: &str,
        sample_rate: u32,
        channels: u32,
        device: &str,
    ) -> Option<u32>;
    fn close_mic_tap(&self, id: u32);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};

    use super::*;

    pub(crate) struct FakeMedia {
        pub log: Mutex<Vec<Value>>,
        pub outputs: Mutex<Vec<AudioOutput>>,
        pub opened: broadcast::Sender<AudioOutput>,
        pub refuse_mic: AtomicBool,
        next_tap: AtomicU32,
    }

    impl FakeMedia {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                log: Mutex::new(Vec::new()),
                outputs: Mutex::new(Vec::new()),
                opened: broadcast::channel(16).0,
                refuse_mic: AtomicBool::new(false),
                next_tap: AtomicU32::new(1),
            })
        }

        pub(crate) fn take(&self) -> Vec<Value> {
            std::mem::take(&mut *self.log.lock().unwrap())
        }

        fn note(&self, v: Value) {
            self.log.lock().unwrap().push(v);
        }
    }

    impl AaMedia for FakeMedia {
        async fn feed_path(&self) -> String {
            "/tmp/gst.feed".to_string()
        }

        fn video_feed(&self, _session: SessionId, cluster: bool) -> u32 {
            if cluster { 7 } else { 1 }
        }

        fn prime_video(&self, _session: SessionId, cluster: bool, _codec: VideoCodec) {
            self.note(json!({ "primeVideo": cluster }));
        }

        fn video_started(&self, cluster: bool, width: u32, height: u32) {
            self.note(json!({ "videoStarted": [cluster, width, height] }));
        }

        fn audio_feed(&self, _session: SessionId, stream: u32) -> u32 {
            stream
        }

        fn set_active(&self, session: SessionId, active: bool) {
            self.note(json!({ "active": [session, active] }));
        }

        fn release(&self, session: SessionId) {
            self.note(json!({ "release": session }));
        }

        fn audio_outputs(&self) -> Vec<AudioOutput> {
            self.outputs.lock().unwrap().clone()
        }

        fn audio_output_opened(&self) -> broadcast::Receiver<AudioOutput> {
            self.opened.subscribe()
        }

        fn prime_audio(&self, kind: AudioKind, sample_rate: u32, channels: u32, tag: &str) {
            self.note(json!({ "primeAudio": [kind as u8, sample_rate, channels, tag] }));
        }

        fn set_host_volume(&self, kind: AudioKind, level: f64, ramp_ms: u32) {
            self.note(json!({ "volume": [kind as u8, level, ramp_ms] }));
        }

        fn open_mic_tap(
            &self,
            path: &str,
            sample_rate: u32,
            channels: u32,
            device: &str,
        ) -> Option<u32> {
            let mut opts = json!({ "sampleRate": sample_rate, "channels": channels });
            if !device.is_empty() {
                opts["device"] = json!(device);
            }
            self.note(json!({ "micTap": [path, opts] }));
            if self.refuse_mic.load(Ordering::Relaxed) {
                return None;
            }
            Some(self.next_tap.fetch_add(1, Ordering::Relaxed))
        }

        fn close_mic_tap(&self, _id: u32) {
            self.note(json!({ "micTapClose": true }));
        }
    }
}
