//! The helper feeds frames and samples straight into the host, a session only
//! sets up where they go.

use std::future::Future;

use tokio::sync::broadcast;

use crate::channels::audio::AudioChannelType;
use crate::discovery::VideoCodec;

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
    /// The channel it was opened for: media, speech, system or telephony.
    pub tag: Option<String>,
}

pub trait AaMedia: Send + Sync + 'static {
    /// Empty when the host cannot provide one.
    fn feed_path(&self) -> impl Future<Output = String> + Send;
    fn video_plane(&self, cluster: bool) -> u32;
    /// Creates the plane, so the fed frames find a decoder.
    fn prime_video(&self, cluster: bool, codec: VideoCodec);
    fn video_started(&self, cluster: bool, width: u32, height: u32);
    /// A held session's frames stop in the host.
    fn set_video_active(&self, cluster: bool, active: bool);
    fn set_audio_active(&self, active: bool);
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

        fn video_plane(&self, cluster: bool) -> u32 {
            if cluster { 7 } else { 1 }
        }

        fn prime_video(&self, cluster: bool, _codec: VideoCodec) {
            self.note(json!({ "primeVideo": cluster }));
        }

        fn video_started(&self, cluster: bool, width: u32, height: u32) {
            self.note(json!({ "videoStarted": [cluster, width, height] }));
        }

        fn set_video_active(&self, cluster: bool, active: bool) {
            self.note(json!({ "videoActive": [cluster, active] }));
        }

        fn set_audio_active(&self, active: bool) {
            self.note(json!({ "audioActive": active }));
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
