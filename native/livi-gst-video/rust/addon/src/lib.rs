//! N-API surface for the gst_video addon: everything here converts between
//! JavaScript values and the crates the pipeline lives in. The feed the helper
//! streams media into ends here as well on the platforms without a host process.

mod feed;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use napi::bindgen_prelude::{Buffer, External};
use napi_derive::napi;

use feed::AudioOut;
use livi_audio_player::uplink::SocketTap;
use livi_audio_player::{Config as AudioConfig, Player as AudioPlayer};
use livi_audio_stream::Codec as AudioCodec;
use livi_video_player::Player;

unsafe extern "C" {
    // The window backdrop is platform code in gst_video_mac.mm.
    #[cfg(not(target_os = "linux"))]
    fn livi_set_backdrop(parent: usize, r: f64, g: f64, b: f64);
}

#[napi(object)]
pub struct CodecSupport {
    pub hw: bool,
    pub sw: bool,
}

#[napi(object)]
pub struct CodecProbe {
    pub h264: CodecSupport,
    pub h265: CodecSupport,
    pub vp9: CodecSupport,
    pub av1: CodecSupport,
}

/// A pipeline the main process holds. With a plane id it is also on the feed's map.
pub struct Plane {
    id: Option<u32>,
    player: Arc<Player>,
}

/// An audio stream the main process holds, on the feed's map under its id.
pub struct AudioStream(Arc<AudioOut>);

/// The GStreamer the pipeline runs on.
#[napi]
pub fn version() -> String {
    livi_video_player::version()
}

fn probe(codec: &str) -> CodecSupport {
    let (hw, sw) = livi_video_player::probe(codec);
    CodecSupport { hw, sw }
}

/// Whether a hardware and a software decoder exist, per codec.
#[napi]
pub fn probe_codecs() -> CodecProbe {
    CodecProbe {
        h264: probe("h264"),
        h265: probe("h265"),
        vp9: probe("vp9"),
        av1: probe("av1"),
    }
}

/// The first pointer-sized bytes of the buffer, which carry the window handle.
fn window_handle(buf: &Buffer) -> usize {
    let bytes: &[u8] = buf.as_ref();
    if bytes.len() < core::mem::size_of::<usize>() {
        return 0;
    }
    let mut raw = [0u8; core::mem::size_of::<usize>()];
    raw.copy_from_slice(&bytes[..core::mem::size_of::<usize>()]);
    usize::from_ne_bytes(raw)
}

/// Builds the pipeline for `codec` into the given window. Null when it fails.
#[napi]
pub fn create_player(
    codec: String,
    window_handle_buf: Buffer,
    codec_data: Option<Buffer>,
    plane_id: Option<u32>,
) -> Option<External<Plane>> {
    let cd: &[u8] = codec_data.as_ref().map_or(&[], |b| b.as_ref());
    let player = Player::new(&codec, window_handle(&window_handle_buf), cd)?;
    Some(External::new(Plane { id: plane_id, player: Arc::new(player) }))
}

/// Starts the pipeline. A plane with an id takes the fed stream from then on.
#[napi]
pub fn start(plane: External<Plane>) {
    plane.player.start();
    if let Some(id) = plane.id {
        feed::register_plane(id, plane.player.clone());
    }
}

/// Feeds one buffer. False when the player cannot take it.
#[napi]
pub fn push_buffer(plane: External<Plane>, buffer: Buffer) -> bool {
    let bytes: &[u8] = buffer.as_ref();
    if bytes.is_empty() {
        return false;
    }
    plane.player.push(bytes)
}

#[napi]
pub fn set_visible(plane: External<Plane>, visible: bool) {
    plane.player.set_visible(visible)
}

#[napi]
pub fn stop(plane: External<Plane>) {
    if let Some(id) = plane.id {
        feed::unregister_plane(id);
    }
    plane.player.stop()
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn set_content_region(
    plane: External<Plane>,
    crop_l: f64,
    crop_t: f64,
    vis_w: f64,
    vis_h: f64,
    tier_w: f64,
    tier_h: f64,
) {
    plane.player.set_content_region(crop_l, crop_t, vis_w, vis_h, tier_w, tier_h)
}

#[napi]
pub fn set_backdrop(window_handle_buf: Buffer, r: f64, g: f64, b: f64) {
    let handle = window_handle(&window_handle_buf);
    if handle == 0 {
        return;
    }
    #[cfg(not(target_os = "linux"))]
    unsafe {
        livi_set_backdrop(handle, r, g, b)
    };
    #[cfg(target_os = "linux")]
    let _ = (r, g, b);
}

#[napi]
pub fn set_gamma(plane: External<Plane>, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
    plane.player.set_gamma(gamma, contrast, r, g, b)
}

/// A running microphone tap, its samples go to the socket for as long as it is held.
pub struct MicTap(SocketTap);

/// Captures the microphone in the given format and streams it into the socket at `path`.
#[napi]
pub fn open_mic_tap(
    path: String,
    sample_rate: u32,
    channels: u32,
    device: Option<String>,
) -> Option<External<MicTap>> {
    SocketTap::open(&path, sample_rate, channels as u8, device.as_deref(), "tap")
        .map(|tap| External::new(MicTap(tap)))
}

#[napi]
pub fn close_mic_tap(tap: External<MicTap>) {
    tap.0.stop()
}

/// Binds the socket the helper streams media into. False when it cannot.
#[napi]
pub fn open_feed(path: String) -> bool {
    match feed::open(&path) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("[feed] cannot bind {path}: {e}");
            false
        }
    }
}

/// Opens a raw PCM stream to the default or the named output. Null when it fails.
#[napi]
pub fn open_audio(
    sample_rate: u32,
    channels: u32,
    device: Option<String>,
    realtime: Option<bool>,
) -> Option<External<AudioStream>> {
    let cfg = AudioConfig {
        codec: AudioCodec::PcmLe,
        payload_type: 0,
        clock_rate: sample_rate,
        channels: channels as u8,
        latency_ms: 0,
        realtime: realtime.unwrap_or(false),
        device,
        label: "fed".into(),
    };
    let player = AudioPlayer::new(&cfg)?;
    player.start();
    Some(External::new(AudioStream(feed::register_audio(player))))
}

/// The id the helper addresses this stream by in the feed.
#[napi]
pub fn audio_stream_id(stream: External<AudioStream>) -> u32 {
    stream.0.id
}

/// Feeds samples from the main process. False when the source no longer takes them.
#[napi]
pub fn push_audio(stream: External<AudioStream>, buffer: Buffer) -> bool {
    stream.0.player.push_samples(buffer.as_ref())
}

/// Whether fed samples reach the sink. A held session keeps its stream silent.
#[napi]
pub fn set_audio_active(stream: External<AudioStream>, active: bool) {
    stream.0.active.store(active, Ordering::Relaxed)
}

#[napi]
pub fn set_audio_volume(stream: External<AudioStream>, level: f64, ramp_ms: u32) {
    stream.0.player.set_volume(level, u64::from(ramp_ms))
}

#[napi]
pub fn close_audio(stream: External<AudioStream>) {
    feed::unregister_audio(stream.0.id);
    stream.0.active.store(false, Ordering::Relaxed);
    stream.0.player.stop()
}
