//! The GStreamer side of the video path.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base as gst_base;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialises GStreamer once, and turns on the debug categories named in
/// `LIVI_GST_DEBUG`. The bare value `1` stands for the decoder and sink
/// categories the video path is usually debugged with.
pub fn ensure_init() {
    INIT.call_once(|| {
        if gst::init().is_err() {
            return;
        }
        if let Ok(spec) = std::env::var("LIVI_GST_DEBUG") {
            let spec = if spec == "1" {
                "v4l2codecs-decoder:6,v4l2codecs-h265dec:6,waylandsink:5,wl_dmabuf:6"
            } else {
                spec.as_str()
            };
            gst::log::set_threshold_from_string(spec, false);
        }
    });
}

/// Prints errors and warnings the pipeline reports, and leaves the message on
/// the bus for anyone else.
pub fn log_bus_messages(pipeline: &gst::Pipeline) {
    let Some(bus) = pipeline.bus() else { return };
    bus.set_sync_handler(|_, msg| {
        let src = msg.src().map(|s| s.name().to_string()).unwrap_or_default();
        match msg.view() {
            gst::MessageView::Error(e) => {
                eprintln!(
                    "[gst_video] ERROR from {src}: {} | {}",
                    e.error(),
                    e.debug().unwrap_or_default()
                );
            }
            gst::MessageView::Warning(w) => {
                eprintln!(
                    "[gst_video] WARN from {src}: {} | {}",
                    w.error(),
                    w.debug().unwrap_or_default()
                );
            }
            _ => {}
        }
        gst::BusSyncReply::Pass
    });
}

/// Takes every sink out of clock sync: the phone's stream is the clock, so a
/// sink that waits on timestamps only adds latency and drops frames on QoS.
pub fn force_sinks_realtime(pipeline: &gst::Pipeline) {
    for element in pipeline.iterate_recurse().into_iter().flatten() {
        if element.is::<gst_base::BaseSink>() {
            element.set_property("sync", false);
            element.set_property("qos", false);
            element.set_property("max-lateness", 0i64);
        }
    }
}

/// Colorimetry the Pi 4 stateful v4l2 decoders reject, and what they accept.
const BAD_COLORIMETRY: &str = "1:4:5:1";
const GOOD_COLORIMETRY: &str = "1:4:7:1";

/// Decoders that need the colorimetry rewritten; the Pi 5 stateless ones do not.
const NEEDS_COLORIMETRY_FIXUP: [&str; 2] = ["v4l2h264dec", "v4l2h265dec"];

const CAL_FRAGMENT: &str = "#version 100
precision highp float;
varying vec2 v_texcoord;
uniform sampler2D tex;
uniform float u_gamma;
uniform float u_contrast;
uniform float u_gain_r;
uniform float u_gain_g;
uniform float u_gain_b;
void main() {
  vec3 c = texture2D(tex, v_texcoord).rgb;
  c = pow(c, vec3(1.0 / u_gamma));
  c = (c - 0.5) * u_contrast + 0.5;
  c = c * vec3(u_gain_r, u_gain_g, u_gain_b);
  gl_FragColor = vec4(clamp(c, 0.0, 1.0), 1.0);
}
";

fn colorimetry(caps: &gst::CapsRef) -> Option<String> {
    let s = caps.structure(0)?;
    s.get::<String>("colorimetry").ok()
}

/// Logs the decoder's negotiated output caps once per caps change.
fn log_decoded_caps(caps: &gst::CapsRef) {
    let Some(s) = caps.structure(0) else { return };
    let fmt = s.get::<String>("format").unwrap_or_else(|_| "?".to_owned());
    let drm = s.get::<String>("drm-format").map(|d| format!(" drm={d}")).unwrap_or_default();
    let w = s.get::<i32>("width").unwrap_or(0);
    let h = s.get::<i32>("height").unwrap_or(0);
    let mem = caps
        .features(0)
        .map(|f| f.to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "SystemMemory".to_owned());
    eprintln!("[gst_video] decoded format={fmt}{drm} {w}x{h} mem={mem}");
}

/// Installs the probes the decoder needs: one that logs its negotiated output,
/// and, for the decoders that reject it, one pair that rewrites the colorimetry
/// the phone announces. Metadata only, the frames are untouched.
fn install_decoder_probes(dec: &gst::Element, decoder_name: &str) {
    if let Some(src) = dec.static_pad("src") {
        src.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, |_, info| {
            if let Some(gst::PadProbeData::Event(ev)) = &info.data {
                if let gst::EventView::Caps(c) = ev.view() {
                    log_decoded_caps(c.caps());
                }
            }
            gst::PadProbeReturn::Ok
        });
    }

    if !NEEDS_COLORIMETRY_FIXUP.contains(&decoder_name) {
        return;
    }
    let Some(sink) = dec.static_pad("sink") else { return };

    sink.add_probe(gst::PadProbeType::QUERY_DOWNSTREAM, |pad, info| {
        let Some(gst::PadProbeData::Query(query)) = &mut info.data else {
            return gst::PadProbeReturn::Ok;
        };
        match query.view_mut() {
            gst::QueryViewMut::Caps(q) => {
                let Some(filter) = q.filter().map(|c| c.to_owned()) else {
                    return gst::PadProbeReturn::Ok;
                };
                if colorimetry(&filter).as_deref() != Some(BAD_COLORIMETRY) {
                    return gst::PadProbeReturn::Ok;
                }
                let Some(tmpl) = pad.pad_template_caps().into() else {
                    return gst::PadProbeReturn::Ok;
                };
                q.set_result(&filter.intersect(&tmpl));
                gst::PadProbeReturn::Handled
            }
            gst::QueryViewMut::AcceptCaps(q) => {
                let Some(caps) = q.caps_owned().into() else {
                    return gst::PadProbeReturn::Ok;
                };
                if colorimetry(&caps).as_deref() != Some(BAD_COLORIMETRY) {
                    return gst::PadProbeReturn::Ok;
                }
                q.set_result(true);
                gst::PadProbeReturn::Handled
            }
            _ => gst::PadProbeReturn::Ok,
        }
    });

    sink.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, |_, info| {
        let Some(gst::PadProbeData::Event(ev)) = &info.data else {
            return gst::PadProbeReturn::Ok;
        };
        let gst::EventView::Caps(c) = ev.view() else {
            return gst::PadProbeReturn::Ok;
        };
        if colorimetry(c.caps()).as_deref() != Some(BAD_COLORIMETRY) {
            return gst::PadProbeReturn::Ok;
        }
        let mut fixed = c.caps().to_owned();
        fixed.get_mut().unwrap().set("colorimetry", GOOD_COLORIMETRY);
        eprintln!(
            "[gst_video] colorimetry {BAD_COLORIMETRY} -> {GOOD_COLORIMETRY} (pi4 v4l2 decoder)"
        );
        info.data = Some(gst::PadProbeData::Event(gst::event::Caps::new(&fixed)));
        gst::PadProbeReturn::Ok
    });
}

// The window view is platform code and stays where it is; these are its entry
// points in gst_video_mac.mm.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn livi_attach_view(parent: usize, out_view: *mut *mut core::ffi::c_void) -> usize;
    fn livi_remove_view(view: *mut core::ffi::c_void);
    fn livi_set_view_hidden(view: *mut core::ffi::c_void, hidden: bool);
    fn livi_set_content_region(
        view: *mut core::ffi::c_void,
        sink: *mut core::ffi::c_void,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    );
}

/// One decoded stream: its pipeline, the source it is fed through, and the
/// window view it draws into.
pub struct Player {
    pipeline: gst::Pipeline,
    appsrc: Option<gst_app::AppSrc>,
    glshader: Option<gst::Element>,
    /// macOS draws into a window of its own: the sink takes its handle, the
    /// view carries position and visibility.
    #[cfg(target_os = "macos")]
    sink: Option<gst::Element>,
    #[cfg(target_os = "macos")]
    view: *mut core::ffi::c_void,
}

unsafe impl Send for Player {}

impl Player {
    /// Builds the pipeline for `codec` and hangs it in the window `handle`
    /// names. None when no decoder is registered or the description fails.
    pub fn new(codec: &str, handle: usize, codec_data: &[u8]) -> Option<Self> {
        ensure_init();

        let mut names = [core::ptr::null(); 8];
        let n = unsafe {
            let c = std::ffi::CString::new(codec).ok()?;
            livi_video_codec::cp_decoder_candidates(
                c.as_ptr(),
                std::env::var_os("LIVI_GST_SWDEC").is_some(),
                names.as_mut_ptr(),
                names.len(),
            )
        };
        let decoder = names[..n]
            .iter()
            .map(|p| unsafe { core::ffi::CStr::from_ptr(*p) }.to_str().unwrap_or(""))
            .find(|name| gst::ElementFactory::find(name).is_some());
        let Some(decoder) = decoder else {
            eprintln!(
                "[gst_video] no decoder registered for {codec}. Install the GStreamer plugin \
                 providing it, software decoding needs gstreamer1.0-libav ({}).",
                livi_video_codec::sw_decoder_for(codec).to_str().unwrap_or("")
            );
            return None;
        };

        let with_cal = !livi_video_codec::calibration().is_empty();
        let pipeline = Self::parse(codec, decoder, with_cal)?;

        let appsrc = pipeline.by_name("src").and_then(|e| e.downcast::<gst_app::AppSrc>().ok());
        if let Some(src) = &appsrc {
            if !codec_data.is_empty() && (codec == "h265" || codec == "h264") {
                src.set_caps(Some(&length_prefixed_caps(codec, codec_data)));
            }
        }

        let mut player = Self {
            glshader: pipeline.by_name("cal"),
            #[cfg(target_os = "macos")]
            sink: pipeline.by_name("sink"),
            #[cfg(target_os = "macos")]
            view: core::ptr::null_mut(),
            appsrc,
            pipeline,
        };

        if player.glshader.is_some() {
            player.glshader.as_ref().unwrap().set_property("fragment", CAL_FRAGMENT);
            player.set_gamma(1.0, 1.0, 1.0, 1.0, 1.0);
        }

        force_sinks_realtime(&player.pipeline);
        if let Some(dec) = player.pipeline.by_name("dec") {
            install_decoder_probes(&dec, decoder);
        }
        log_bus_messages(&player.pipeline);
        player.attach_view(handle);

        Some(player)
    }

    /// Parses the description, and drops the calibration pass when the platform
    /// announces one the elements cannot deliver.
    fn parse(codec: &str, decoder: &str, with_cal: bool) -> Option<gst::Pipeline> {
        let describe = |cal: bool| {
            livi_video_codec::pipeline_desc(
                codec,
                decoder,
                if cal { livi_video_codec::calibration() } else { "" },
                std::env::var("LIVI_GST_SINK").ok().as_deref(),
            )
        };

        let desc = describe(with_cal);
        eprintln!(
            "[gst_video] codec={codec} decoder={decoder} ({}) | {desc}",
            if livi_video_codec::is_hw_decoder(decoder) { "hw" } else { "sw" }
        );

        match gst::parse::launch(&desc) {
            Ok(p) => p.downcast::<gst::Pipeline>().ok(),
            Err(e) if with_cal => {
                eprintln!("[gst_video] calibration pass failed ({e}), retrying without it");
                let plain = describe(false);
                gst::parse::launch(&plain)
                    .inspect_err(|e| eprintln!("[gst_video] pipeline parse FAILED: {e}"))
                    .ok()?
                    .downcast::<gst::Pipeline>()
                    .ok()
            }
            Err(e) => {
                eprintln!("[gst_video] pipeline parse FAILED: {e}");
                None
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn attach_view(&mut self, _handle: usize) {}

    #[cfg(target_os = "macos")]
    fn attach_view(&mut self, handle: usize) {
        use gstreamer_video::prelude::VideoOverlayExtManual;
        if handle == 0 {
            return;
        }
        let overlay = unsafe { livi_attach_view(handle, &mut self.view) };
        if overlay == 0 {
            return;
        }
        if let Some(o) = self.sink.as_ref().and_then(|s| s.dynamic_cast_ref::<gstreamer_video::VideoOverlay>()) {
            unsafe { o.set_window_handle(overlay) };
        }
    }

    pub fn start(&self) {
        let _ = self.pipeline.set_state(gst::State::Playing);
    }

    pub fn stop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.remove_view();
    }

    /// Feeds one buffer; false when there is no source to take it.
    pub fn push(&self, data: &[u8]) -> bool {
        let Some(src) = &self.appsrc else { return false };
        if data.is_empty() {
            return false;
        }
        src.push_buffer(gst::Buffer::from_slice(data.to_vec())).is_ok()
    }

    pub fn set_visible(&self, visible: bool) {
        #[cfg(target_os = "macos")]
        unsafe {
            livi_set_view_hidden(self.view, !visible)
        };
        #[cfg(not(target_os = "macos"))]
        let _ = visible;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_content_region(
        &self,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    ) {
        #[cfg(target_os = "macos")]
        {
            if self.view.is_null() {
                return;
            }
            let sink = self.sink.as_ref().map_or(core::ptr::null_mut(), |s| {
                s.as_ptr() as *mut core::ffi::c_void
            });
            unsafe {
                livi_set_content_region(self.view, sink, crop_l, crop_t, vis_w, vis_h, tier_w, tier_h)
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (crop_l, crop_t, vis_w, vis_h, tier_w, tier_h);
    }

    /// Steers the calibration shader; without one this does nothing.
    pub fn set_gamma(&self, gamma: f64, contrast: f64, gain_r: f64, gain_g: f64, gain_b: f64) {
        let Some(shader) = &self.glshader else { return };
        let uniforms = gst::Structure::builder("uniforms")
            .field("u_gamma", if gamma > 0.0 { gamma as f32 } else { 1.0 })
            .field("u_contrast", contrast as f32)
            .field("u_gain_r", gain_r as f32)
            .field("u_gain_g", gain_g as f32)
            .field("u_gain_b", gain_b as f32)
            .build();
        shader.set_property("uniforms", uniforms);
    }

    fn remove_view(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.view.is_null() {
            unsafe { livi_remove_view(self.view) };
            self.view = core::ptr::null_mut();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.remove_view();
    }
}

/// CarPlay ships the parameter sets once as an hvcC/avcC record and every frame
/// as length-prefixed NALs, which is the hvc1/avc stream format.
fn length_prefixed_caps(codec: &str, codec_data: &[u8]) -> gst::Caps {
    let (media, stream_format) =
        if codec == "h265" { ("video/x-h265", "hvc1") } else { ("video/x-h264", "avc") };
    gst::Caps::builder(media)
        .field("stream-format", stream_format)
        .field("alignment", "au")
        .field("codec_data", gst::Buffer::from_slice(codec_data.to_vec()))
        .build()
}

/// The GStreamer the pipeline runs on.
pub fn version() -> String {
    ensure_init();
    gst::version_string().to_string()
}

/// Whether a hardware and a software decoder are registered for `codec`.
pub fn probe(codec: &str) -> (bool, bool) {
    ensure_init();
    let exists = |name: &str| gst::ElementFactory::find(name).is_some();

    let mut names = [core::ptr::null(); 8];
    let Ok(c) = std::ffi::CString::new(codec) else { return (false, false) };
    let n = unsafe {
        livi_video_codec::cp_decoder_candidates(c.as_ptr(), false, names.as_mut_ptr(), names.len())
    };
    let best = names[..n]
        .iter()
        .map(|p| unsafe { core::ffi::CStr::from_ptr(*p) }.to_str().unwrap_or(""))
        .find(|name| exists(name));

    let hw = best.is_some_and(livi_video_codec::is_hw_decoder);
    let sw = exists(livi_video_codec::sw_decoder_for(codec).to_str().unwrap_or(""));
    (hw, sw)
}

/// # Safety
/// `codec` is a NUL-terminated string, `codec_data` points to `codec_data_len`
/// readable bytes. The result is freed with `cp_player_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_player_new(
    codec: *const core::ffi::c_char,
    handle: usize,
    codec_data: *const u8,
    codec_data_len: usize,
) -> *mut Player {
    let name = if codec.is_null() {
        "h264".to_owned()
    } else {
        unsafe { core::ffi::CStr::from_ptr(codec) }.to_string_lossy().into_owned()
    };
    let cd = if codec_data.is_null() || codec_data_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(codec_data, codec_data_len) }
    };
    match Player::new(&name, handle, cd) {
        Some(p) => Box::into_raw(Box::new(p)),
        None => core::ptr::null_mut(),
    }
}

/// # Safety
/// `p` comes from `cp_player_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_player_free(p: *mut Player) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p) });
    }
}

/// # Safety
/// `p` comes from `cp_player_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_player_start(p: *mut Player) {
    if let Some(p) = unsafe { p.as_ref() } {
        p.start();
    }
}

/// # Safety
/// `p` comes from `cp_player_new`, `data` points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_player_push(p: *mut Player, data: *const u8, len: usize) -> bool {
    let Some(p) = (unsafe { p.as_ref() }) else { return false };
    if data.is_null() || len == 0 {
        return false;
    }
    p.push(unsafe { core::slice::from_raw_parts(data, len) })
}

/// # Safety
/// `p` comes from `cp_player_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_player_set_gamma(
    p: *mut Player,
    gamma: f64,
    contrast: f64,
    gain_r: f64,
    gain_g: f64,
    gain_b: f64,
) {
    if let Some(p) = unsafe { p.as_ref() } {
        p.set_gamma(gamma, contrast, gain_r, gain_g, gain_b);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn cp_gst_ensure_init() {
    ensure_init();
}

/// # Safety
/// `codec` is a NUL-terminated string, `hw` and `sw` are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_gst_probe(codec: *const core::ffi::c_char, hw: *mut bool, sw: *mut bool) {
    let name = if codec.is_null() {
        String::new()
    } else {
        unsafe { core::ffi::CStr::from_ptr(codec) }.to_string_lossy().into_owned()
    };
    let (h, s) = probe(&name);
    unsafe {
        *hw = h;
        *sw = s;
    }
}
