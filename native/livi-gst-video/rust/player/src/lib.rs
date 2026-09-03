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

/// Takes every sink out of clock sync. The phone's stream is the clock.
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

/// Decoders that need the colorimetry rewritten.
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

/// Installs the decoder probes: one logs the negotiated output caps, one pair
/// rewrites the colorimetry the phone announces for the decoders that need it.
fn install_decoder_probes(dec: &gst::Element, decoder_name: &str) {
    if let Some(src) = dec.static_pad("src") {
        src.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, |_, info| {
            if let Some(gst::PadProbeData::Event(ev)) = &info.data
                && let gst::EventView::Caps(c) = ev.view()
            {
                log_decoded_caps(c.caps());
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

// The window view entry points in gst_video_mac.mm.
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
    view: core::sync::atomic::AtomicPtr<core::ffi::c_void>,
}


impl Player {
    /// Builds the pipeline for `codec` and hangs it in the window `handle`
    /// names. None when no decoder is registered or the description fails.
    pub fn new(codec: &str, handle: usize, codec_data: &[u8]) -> Option<Self> {
        ensure_init();

        let sw_only = std::env::var_os("LIVI_GST_SWDEC").is_some();
        let decoder = livi_video_codec::decoder_candidates(codec, sw_only)
            .iter()
            .filter_map(|name| name.to_str().ok())
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
        if let Some(src) = &appsrc
            && !codec_data.is_empty()
            && (codec == "h265" || codec == "h264")
        {
            src.set_caps(Some(&length_prefixed_caps(codec, codec_data)));
        }

        let mut player = Self {
            glshader: pipeline.by_name("cal"),
            #[cfg(target_os = "macos")]
            sink: pipeline.by_name("sink"),
            #[cfg(target_os = "macos")]
            view: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
            appsrc,
            pipeline,
        };

        if let Some(shader) = &player.glshader {
            shader.set_property("fragment", CAL_FRAGMENT);
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
        let mut view = core::ptr::null_mut();
        let overlay = unsafe { livi_attach_view(handle, &mut view) };
        self.view.store(view, core::sync::atomic::Ordering::Release);
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

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.remove_view();
    }

    /// Feeds one buffer. False when there is no source to take it.
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
            livi_set_view_hidden(self.view_ptr(), !visible)
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
            let view = self.view_ptr();
            if view.is_null() {
                return;
            }
            let sink = self.sink.as_ref().map_or(core::ptr::null_mut(), |s| {
                s.as_ptr() as *mut core::ffi::c_void
            });
            unsafe {
                livi_set_content_region(view, sink, crop_l, crop_t, vis_w, vis_h, tier_w, tier_h)
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (crop_l, crop_t, vis_w, vis_h, tier_w, tier_h);
    }

    /// Drops everything queued, so the next frames play without the old tail.
    pub fn flush(&self) {
        let Some(src) = &self.appsrc else { return };
        let _ = src.send_event(gst::event::FlushStart::new());
        let _ = src.send_event(gst::event::FlushStop::new(true));
    }

    /// Steers the calibration shader.
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

    #[cfg(target_os = "macos")]
    fn view_ptr(&self) -> *mut core::ffi::c_void {
        self.view.load(core::sync::atomic::Ordering::Acquire)
    }

    // The view is swapped out atomically, so a shared player can be stopped from any thread.
    fn remove_view(&self) {
        #[cfg(target_os = "macos")]
        {
            let view = self.view.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::AcqRel);
            if !view.is_null() {
                unsafe { livi_remove_view(view) };
            }
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

    let best = livi_video_codec::decoder_candidates(codec, false)
        .iter()
        .filter_map(|name| name.to_str().ok())
        .find(|name| exists(name));

    let hw = best.is_some_and(livi_video_codec::is_hw_decoder);
    let sw = exists(livi_video_codec::sw_decoder_for(codec).to_str().unwrap_or(""));
    (hw, sw)
}
