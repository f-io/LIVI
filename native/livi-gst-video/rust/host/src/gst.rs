//! What the host reaches for when it runs for real: GStreamer pipelines and a
//! listening socket for the phone's screen stream.

use livi_audio_player::uplink::{SocketTap, Uplink, UplinkConfig as UplinkPipeline};
use livi_audio_player::{Config as AudioPipeline, Player as AudioPipelinePlayer};
use livi_audio_stream::AudioSink;
use livi_screen_stream::ScreenSink;
use livi_video_player::Player;

use crate::{AudioConfig, MediaSink, Outside, Plane, Speaker, TapConfig, UplinkConfig};

/// A running microphone tap, its samples go to the socket for as long as it is held.
pub struct Tap(#[allow(dead_code)] SocketTap);

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

/// The socket one screen receiver listens on.
#[cfg(target_os = "linux")]
pub struct Ears(#[allow(dead_code)] livi_screen_stream::receiver::ScreenReceiver);

#[cfg(not(target_os = "linux"))]
pub struct Ears;

/// The data and control sockets one audio stream is bound to.
#[cfg(target_os = "linux")]
pub struct AudioEars(#[allow(dead_code)] livi_audio_stream::receiver::AudioReceiver);

#[cfg(not(target_os = "linux"))]
pub struct AudioEars;

/// The socket the helper's media feed listens on.
#[cfg(target_os = "linux")]
pub struct FeedEars(#[allow(dead_code)] crate::feed::FeedListener);

#[cfg(not(target_os = "linux"))]
pub struct FeedEars;

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
}

pub struct Gst;

impl Outside for Gst {
    type Plane = Player;
    type Ears = Ears;
    type Speaker = AudioPipelinePlayer;
    type AudioEars = AudioEars;
    type Uplink = Uplink;
    type Tap = Tap;

    fn open_tap(&self, cfg: TapConfig) -> Option<Tap> {
        SocketTap::open(&cfg.path, cfg.sample_rate, cfg.channels, cfg.device.as_deref(), "tap").map(Tap)
    }

    fn create_plane(&self, codec: &str, codec_data: &[u8]) -> Option<Player> {
        // the window comes from the sink, so the player needs no handle
        Player::new(codec, 0, codec_data)
    }

    #[cfg(target_os = "linux")]
    fn listen(&self, key: [u8; 32], sink: Box<dyn ScreenSink>) -> Option<(Ears, u16)> {
        match livi_screen_stream::receiver::ScreenReceiver::new(key, sink) {
            Ok((r, port)) => Some((Ears(r), port)),
            Err(e) => {
                eprintln!("[cp_screen] cannot listen: {e}");
                None
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn listen(&self, _key: [u8; 32], _sink: Box<dyn ScreenSink>) -> Option<(Ears, u16)> {
        None
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
        })?;
        player.start();
        Some(player)
    }

    #[cfg(target_os = "linux")]
    fn listen_audio(
        &self,
        key: [u8; 32],
        sink: Box<dyn AudioSink + Send>,
    ) -> Option<(AudioEars, u16, u16)> {
        let stream = livi_audio_stream::AudioStream::new(key, sink);
        match livi_audio_stream::receiver::AudioReceiver::new(stream) {
            Ok((r, data, control)) => Some((AudioEars(r), data, control)),
            Err(e) => {
                eprintln!("[cp_audio] cannot listen: {e}");
                None
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn listen_audio(
        &self,
        _key: [u8; 32],
        _sink: Box<dyn AudioSink + Send>,
    ) -> Option<(AudioEars, u16, u16)> {
        None
    }

    type FeedEars = FeedEars;

    #[cfg(target_os = "linux")]
    fn open_feed(&self, path: &str, sink: Box<dyn MediaSink>) -> Option<FeedEars> {
        match crate::feed::FeedListener::new(path, sink) {
            Ok(l) => Some(FeedEars(l)),
            Err(e) => {
                eprintln!("[feed] cannot listen on {path}: {e}");
                None
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn open_feed(&self, _path: &str, _sink: Box<dyn MediaSink>) -> Option<FeedEars> {
        None
    }
}

/// The codec support the main process picks its decoders from.
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
