//! N-API surface for the gst_video addon. The pipeline itself lives in the C++
//! sources compiled by build.rs; everything here converts between JavaScript
//! values and their C entry points.

#[cfg(target_os = "linux")]
use core::ffi::c_char;
#[cfg(target_os = "linux")]
use std::ffi::CString;
use napi::bindgen_prelude::{Buffer, External};
use napi_derive::napi;

// The element choice, the screen receiver's AEAD, the NAL helpers, the stream
// framing and the fanout live in these crates; the references keep rustc from
// dropping the otherwise-unused dependencies at link time.
use livi_video_codec as _;
use livi_video_player::Player;
#[cfg(target_os = "linux")]
use livi_crypto_node as _;
#[cfg(target_os = "linux")]
use livi_screen_stream as _;
#[cfg(target_os = "linux")]
use livi_host_proto as _;
#[cfg(target_os = "linux")]
use livi_video_fanout as _;
#[cfg(target_os = "linux")]
use livi_video_nal as _;

unsafe extern "C" {
    // The window backdrop is platform code in gst_video_mac.mm.
    #[cfg(not(target_os = "linux"))]
    fn livi_set_backdrop(parent: usize, r: f64, g: f64, b: f64);
    #[cfg(target_os = "linux")]
    fn livi_gst_host_run(sock: *const c_char, crash: *const c_char);
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

/// Builds the pipeline for `codec` into the given window; null when it fails.
#[napi]
pub fn create_player(
    codec: String,
    window_handle_buf: Buffer,
    codec_data: Option<Buffer>,
) -> Option<External<Player>> {
    let cd: &[u8] = codec_data.as_ref().map_or(&[], |b| b.as_ref());
    Player::new(&codec, window_handle(&window_handle_buf), cd).map(External::new)
}

#[napi]
pub fn start(player: External<Player>) {
    player.start()
}

/// Feeds one buffer; false when the player cannot take it.
#[napi]
pub fn push_buffer(player: External<Player>, buffer: Buffer) -> bool {
    let bytes: &[u8] = buffer.as_ref();
    if bytes.is_empty() {
        return false;
    }
    player.push(bytes)
}

#[napi]
pub fn set_visible(player: External<Player>, visible: bool) {
    player.set_visible(visible)
}

#[napi]
pub fn stop(mut player: External<Player>) {
    player.stop()
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn set_content_region(
    player: External<Player>,
    crop_l: f64,
    crop_t: f64,
    vis_w: f64,
    vis_h: f64,
    tier_w: f64,
    tier_h: f64,
) {
    player.set_content_region(crop_l, crop_t, vis_w, vis_h, tier_w, tier_h)
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
pub fn set_gamma(
    player: External<Player>,
    gamma: f64,
    contrast: f64,
    r: f64,
    g: f64,
    b: f64,
) {
    player.set_gamma(gamma, contrast, r, g, b)
}

/// Runs the host process's main loop; returns when it ends.
#[cfg(target_os = "linux")]
#[napi]
pub fn run(sock_path: String, crash_path: String) {
    let sock = CString::new(sock_path).unwrap_or_default();
    let crash = CString::new(crash_path).unwrap_or_default();
    unsafe { livi_gst_host_run(sock.as_ptr(), crash.as_ptr()) }
}
