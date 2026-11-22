// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base as gst_base;
use livi_output_proto::VideoCodec;
use livi_video::nal::{CpCodec, CpNalKind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};

mod camera;
#[cfg(in_process)]
mod in_process_view;
mod url_player;
#[cfg_attr(
    all(target_os = "macos", feature = "window-view"),
    path = "view/mac.rs"
)]
#[cfg_attr(in_process, path = "view/in_process.rs")]
#[cfg_attr(livi_os, path = "view/kms.rs")]
#[cfg_attr(
    not(any(all(target_os = "macos", feature = "window-view"), in_process, livi_os)),
    path = "view/none.rs"
)]
mod view;
pub use camera::{CameraPlayer, direction, watch_cameras};
#[cfg(in_process)]
pub use in_process_view::{Views, set_surface, set_views};
pub use url_player::UrlPlayer;
use view::WindowView;

pub type Say = Arc<dyn Fn(&str) + Send + Sync>;

pub fn to_stderr() -> Say {
    Arc::new(|line| eprintln!("{}", stamped(line)))
}

pub fn stamped(line: &str) -> String {
    let at = gst::glib::DateTime::now_local()
        .and_then(|now| now.format("%H:%M:%S.%f"))
        .map(|t| t.chars().take(12).collect::<String>())
        .unwrap_or_default();
    format!("[{at}] {line}")
}

static INIT: Once = Once::new();

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
        gst::BusSyncReply::Drop
    });
}

pub fn force_sinks_realtime(pipeline: &gst::Pipeline) {
    for element in pipeline.iterate_recurse().into_iter().flatten() {
        if element.is::<gst_base::BaseSink>() {
            element.set_property("sync", false);
            element.set_property("qos", false);
            element.set_property("max-lateness", 0i64);
        }
    }
}

const BAD_COLORIMETRY: &str = "1:4:5:1";
const GOOD_COLORIMETRY: &str = "1:4:7:1";

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

pub(crate) fn decoded_caps(caps: &gst::CapsRef) -> Option<String> {
    let s = caps.structure(0)?;
    let fmt = s.get::<String>("format").unwrap_or_else(|_| "?".to_owned());
    let drm = s
        .get::<String>("drm-format")
        .map(|d| format!(" drm={d}"))
        .unwrap_or_default();
    let w = s.get::<i32>("width").unwrap_or(0);
    let h = s.get::<i32>("height").unwrap_or(0);
    let mem = caps
        .features(0)
        .map(|f| f.to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "SystemMemory".to_owned());
    Some(format!("format={fmt}{drm} {w}x{h} mem={mem}"))
}

fn install_decoder_probes(dec: &gst::Element, decoder_name: &str, say: &Say) {
    if let Some(src) = dec.static_pad("src") {
        let last = std::sync::Mutex::new(String::new());
        let say = say.clone();
        src.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
            if let Some(gst::PadProbeData::Event(ev)) = &info.data
                && let gst::EventView::Caps(c) = ev.view()
                && let Some(now) = decoded_caps(c.caps())
            {
                let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
                if *last != now {
                    say(&format!("[gst_video] decoded {now}"));
                    *last = now;
                }
            }
            gst::PadProbeReturn::Ok
        });
    }

    if !NEEDS_COLORIMETRY_FIXUP.contains(&decoder_name) {
        return;
    }
    let Some(sink) = dec.static_pad("sink") else {
        return;
    };

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
        fixed
            .get_mut()
            .unwrap()
            .set("colorimetry", GOOD_COLORIMETRY);
        eprintln!(
            "[gst_video] colorimetry {BAD_COLORIMETRY} -> {GOOD_COLORIMETRY} (pi4 v4l2 decoder)"
        );
        info.data = Some(gst::PadProbeData::Event(gst::event::Caps::new(&fixed)));
        gst::PadProbeReturn::Ok
    });
}

const RANK_FRAMES: i32 = 0;
const RANK_URL: i32 = 1;
const RANK_CAMERA: i32 = 2;

pub struct Player {
    id: usize,
    codec: VideoCodec,
    decoder: String,
    handle: usize,
    codec_data: Vec<u8>,
    decoding: Mutex<Option<Decoding>>,
    to_keyframe: AtomicBool,
    started: AtomicBool,
    stopped: AtomicBool,
    gamma: Mutex<Option<[f64; 5]>>,
    view: WindowView,
    spare: Spare,
    /// The sender gives the next keyframe only with the next format.
    held: Mutex<Vec<Vec<u8>>>,
}

struct Decoding {
    pipeline: gst::Pipeline,
    appsrc: Option<gst_app::AppSrc>,
    glshader: Option<gst::Element>,
}

impl Decoding {
    fn flush(&self) {
        let Some(src) = &self.appsrc else { return };
        let _ = src.send_event(gst::event::FlushStart::new());
        let _ = src.send_event(gst::event::FlushStop::new(true));
    }

    fn close(self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        PIPELINES.fetch_sub(1, Ordering::Relaxed);
    }
}

type Parked = (VideoCodec, String, Vec<u8>, Decoding);

/// `LIVI_VIDEO_DECODERS` caps the pipelines, parked ones included. Unset or 0 parks none.
static PARKED: Mutex<Vec<Parked>> = Mutex::new(Vec::new());
static PIPELINES: AtomicUsize = AtomicUsize::new(0);
const HELD_MAX: usize = 300;
static PLAYERS: AtomicUsize = AtomicUsize::new(0);

fn decoders_cap() -> usize {
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| {
        std::env::var("LIVI_VIDEO_DECODERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn cp_codec(codec: VideoCodec) -> Option<CpCodec> {
    match codec {
        VideoCodec::H264 => Some(CpCodec::H264),
        VideoCodec::H265 => Some(CpCodec::H265),
        VideoCodec::Vp9 | VideoCodec::Av1 => None,
    }
}

/// Reused: musl hands freed memory back to the kernel, new frames would cost fresh zeroed pages.
type Spare = std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>;
/// About what the queues in front of the decoder hold.
const SPARE: usize = 6;

struct Pooled(Vec<u8>, Spare);

impl AsRef<[u8]> for Pooled {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        let mut spare = self.1.lock().unwrap_or_else(|e| e.into_inner());
        if spare.len() < SPARE {
            spare.push(std::mem::take(&mut self.0));
        }
    }
}

impl Player {
    pub fn new(codec: VideoCodec, handle: usize, codec_data: &[u8]) -> Option<Self> {
        ensure_init();

        let sw_only = std::env::var_os("LIVI_GST_SWDEC").is_some();
        let decoder = candidates(codec, sw_only)
            .into_iter()
            .find(|name| usable(name));
        let Some(decoder) = decoder else {
            eprintln!(
                "[gst_video] no decoder registered for {codec}. Install the GStreamer plugin \
                 providing it, software decoding needs gstreamer1.0-libav ({}).",
                livi_video::codec::sw_decoder_for(codec)
                    .to_str()
                    .unwrap_or("")
            );
            return None;
        };

        let player = Self {
            id: PLAYERS.fetch_add(1, Ordering::Relaxed),
            codec,
            decoder,
            handle,
            codec_data: codec_data.to_vec(),
            decoding: Mutex::new(None),
            to_keyframe: AtomicBool::new(false),
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            gamma: Mutex::new(None),
            view: WindowView::new(),
            spare: Spare::default(),
            held: Mutex::new(Vec::new()),
        };
        if !player.acquire()? {
            eprintln!(
                "[gst_video] player {} waits for a {codec} decoder to be handed back",
                player.id
            );
        }
        Some(player)
    }

    /// Some(false): every decoder the cap allows is in use. None: no pipeline could be built.
    fn acquire(&self) -> Option<bool> {
        let mut decoding = locked(&self.decoding);
        if decoding.is_some() {
            return Some(true);
        }
        let parked = {
            let mut parked = locked(&PARKED);
            match parked.iter().position(|(c, d, data, _)| {
                *c == self.codec && *d == self.decoder && *data == self.codec_data
            }) {
                Some(i) => Some(parked.remove(i).3),
                None => {
                    // Its buffers have another format's size, the new ones need their room.
                    while let Some(i) = parked
                        .iter()
                        .position(|(c, d, _, _)| *c == self.codec && *d == self.decoder)
                    {
                        parked.remove(i).3.close();
                    }
                    let cap = decoders_cap();
                    while cap > 0 && PIPELINES.load(Ordering::Relaxed) >= cap && !parked.is_empty()
                    {
                        parked.remove(0).3.close();
                    }
                    None
                }
            }
        };
        let reused = parked.is_some();
        let d = match parked {
            Some(d) => d,
            None => {
                let cap = decoders_cap();
                if cap > 0 && PIPELINES.load(Ordering::Relaxed) >= cap {
                    return Some(false);
                }
                let d = self.build()?;
                PIPELINES.fetch_add(1, Ordering::Relaxed);
                d
            }
        };
        if let Some(src) = &d.appsrc
            && !self.codec_data.is_empty()
            && matches!(self.codec, VideoCodec::H264 | VideoCodec::H265)
        {
            src.set_caps(Some(&length_prefixed_caps(self.codec, &self.codec_data)));
        }
        #[cfg(target_os = "android")]
        if let Some(src) = &d.appsrc
            && let Some(caps) = src.caps()
        {
            src.set_caps(Some(&with_variable_rate(caps)));
        }
        if let Some(g) = *locked(&self.gamma) {
            apply_gamma(&d, g);
        }
        if reused {
            eprintln!(
                "[gst_video] player {} takes over the parked {} pipeline",
                self.id, self.decoder
            );
        }
        self.view.attach(
            self.handle,
            RANK_FRAMES,
            d.pipeline.by_name("sink").as_ref(),
        );
        if self.started.load(Ordering::Relaxed) {
            let _ = d.pipeline.set_state(gst::State::Playing);
        }
        self.to_keyframe.store(true, Ordering::Relaxed);
        *decoding = Some(d);
        Some(true)
    }

    fn park(&self) {
        let Some(d) = locked(&self.decoding).take() else {
            return;
        };
        self.view.remove();
        if decoders_cap() == 0 {
            d.close();
            return;
        }
        d.flush();
        let mut parked = locked(&PARKED);
        if parked
            .iter()
            .any(|(c, n, _, _)| *c == self.codec && *n == self.decoder)
        {
            drop(parked);
            d.close();
        } else {
            parked.push((self.codec, self.decoder.clone(), self.codec_data.clone(), d));
        }
    }

    fn build(&self) -> Option<Decoding> {
        let with_cal = !livi_video::codec::calibration().is_empty();
        let pipeline = Self::parse(self.codec, &self.decoder, with_cal, self.handle)?;
        let d = Decoding {
            appsrc: pipeline
                .by_name("src")
                .and_then(|e| e.downcast::<gst_app::AppSrc>().ok()),
            glshader: pipeline.by_name("cal"),
            pipeline,
        };
        if let Some(shader) = &d.glshader {
            shader.set_property("fragment", CAL_FRAGMENT);
            apply_gamma(&d, [1.0; 5]);
        }
        force_sinks_realtime(&d.pipeline);
        if let Some(dec) = d.pipeline.by_name("dec") {
            if cfg!(target_vendor = "apple") && self.decoder == "avdec_h265" {
                dec.set_property_from_str("thread-type", "frame");
                dec.set_property("max-threads", 1i32);
                eprintln!(
                    "[gst_video] {} thread-type={} max-threads={}",
                    self.decoder,
                    dec.property_value("thread-type")
                        .serialize()
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    dec.property::<i32>("max-threads")
                );
            }
            install_decoder_probes(&dec, &self.decoder, &to_stderr());
        }
        log_bus_messages(&d.pipeline);
        Some(d)
    }

    fn parse(
        codec: VideoCodec,
        decoder: &str,
        with_cal: bool,
        handle: usize,
    ) -> Option<gst::Pipeline> {
        let describe = |cal: bool| {
            livi_video::codec::pipeline_desc(
                codec,
                decoder,
                if cal {
                    livi_video::codec::calibration()
                } else {
                    ""
                },
                livi_video::codec::sink_override().as_deref(),
            )
        };

        let desc = describe(with_cal);
        eprintln!(
            "[gst_video] window={handle:#x} codec={codec} decoder={decoder} ({}) | {desc}",
            if livi_video::codec::is_hw_decoder(decoder) {
                "hw"
            } else {
                "sw"
            }
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

    pub fn start(&self) {
        self.started.store(true, Ordering::Relaxed);
        if let Some(d) = &*locked(&self.decoding) {
            let _ = d.pipeline.set_state(gst::State::Playing);
        }
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.park();
    }

    pub fn push(&self, data: &[u8]) -> bool {
        if data.is_empty() || self.stopped.load(Ordering::Relaxed) {
            return false;
        }
        if locked(&self.decoding).is_none() {
            if self.acquire() != Some(true) {
                self.hold(data);
                return true;
            }
            self.replay();
        }
        if self.to_keyframe.load(Ordering::Relaxed)
            && let Some(cp) = cp_codec(self.codec)
        {
            match livi_video::nal::classify_nal(data, cp) {
                CpNalKind::Keyframe => self.to_keyframe.store(false, Ordering::Relaxed),
                CpNalKind::Delta => return true,
                CpNalKind::Params => {}
            }
        }
        match &*locked(&self.decoding) {
            Some(d) => self.feed(d, data),
            None => false,
        }
    }

    fn hold(&self, data: &[u8]) {
        let Some(cp) = cp_codec(self.codec) else {
            return;
        };
        let kind = |d: &[u8]| livi_video::nal::classify_nal(d, cp);
        let mut held = locked(&self.held);
        let keyed = held.iter().any(|h| kind(h) == CpNalKind::Keyframe);
        match kind(data) {
            CpNalKind::Params if keyed => held.clear(),
            CpNalKind::Params => {}
            CpNalKind::Keyframe => {
                let params = held.iter().rposition(|h| kind(h) != CpNalKind::Params);
                held.drain(..params.map_or(0, |i| i + 1));
            }
            CpNalKind::Delta if keyed && held.len() < HELD_MAX => {}
            CpNalKind::Delta => return,
        }
        held.push(data.to_vec());
    }

    fn replay(&self) {
        let held = std::mem::take(&mut *locked(&self.held));
        let Some(cp) = cp_codec(self.codec) else {
            return;
        };
        if !held
            .iter()
            .any(|h| livi_video::nal::classify_nal(h, cp) == CpNalKind::Keyframe)
        {
            return;
        }
        if let Some(d) = &*locked(&self.decoding) {
            self.to_keyframe.store(false, Ordering::Relaxed);
            held.iter().for_each(|h| {
                self.feed(d, h);
            });
        }
    }

    fn feed(&self, d: &Decoding, data: &[u8]) -> bool {
        let Some(src) = &d.appsrc else {
            return false;
        };
        let mut bytes = self
            .spare
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop()
            .unwrap_or_default();
        bytes.clear();
        bytes.extend_from_slice(data);
        src.push_buffer(gst::Buffer::from_slice(Pooled(bytes, self.spare.clone())))
            .is_ok()
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn set_visible(&self, visible: bool) {
        self.view.set_visible(visible);
    }

    pub fn set_content_region(
        &self,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    ) {
        self.view
            .set_region(crop_l, crop_t, vis_w, vis_h, tier_w, tier_h);
    }

    pub fn flush(&self) {
        if let Some(d) = &*locked(&self.decoding) {
            d.flush();
            self.to_keyframe.store(true, Ordering::Relaxed);
        }
    }

    pub fn set_gamma(&self, gamma: f64, contrast: f64, gain_r: f64, gain_g: f64, gain_b: f64) {
        let g = [gamma, contrast, gain_r, gain_g, gain_b];
        *locked(&self.gamma) = Some(g);
        if let Some(d) = &*locked(&self.decoding) {
            apply_gamma(d, g);
        }
    }
}

fn apply_gamma(d: &Decoding, [gamma, contrast, gain_r, gain_g, gain_b]: [f64; 5]) {
    let Some(shader) = &d.glshader else { return };
    let uniforms = gst::Structure::builder("uniforms")
        .field("u_gamma", if gamma > 0.0 { gamma as f32 } else { 1.0 })
        .field("u_contrast", contrast as f32)
        .field("u_gain_r", gain_r as f32)
        .field("u_gain_g", gain_g as f32)
        .field("u_gain_b", gain_b as f32)
        .build();
    shader.set_property("uniforms", uniforms);
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "android")]
fn with_variable_rate(mut caps: gst::Caps) -> gst::Caps {
    caps.make_mut().set("framerate", gst::Fraction::new(0, 1));
    caps
}

fn length_prefixed_caps(codec: VideoCodec, codec_data: &[u8]) -> gst::Caps {
    let (media, stream_format) = if codec == VideoCodec::H265 {
        ("video/x-h265", "hvc1")
    } else {
        ("video/x-h264", "avc")
    };
    gst::Caps::builder(media)
        .field("stream-format", stream_format)
        .field("alignment", "au")
        .field("codec_data", gst::Buffer::from_slice(codec_data.to_vec()))
        .build()
}

pub fn version() -> String {
    ensure_init();
    gst::version_string().to_string()
}

fn render_gpu() -> Option<livi_video::codec::RenderGpu> {
    let node = std::env::var("LIVI_RENDER_NODE").ok()?;
    let name = std::path::Path::new(&node)
        .file_name()?
        .to_str()?
        .to_owned();
    let driver = std::fs::read_link(format!("/sys/class/drm/{name}/device/driver")).ok();
    let nvidia = driver
        .as_deref()
        .and_then(|d| d.file_name())
        .is_some_and(|d| d == "nvidia");
    Some(if nvidia {
        livi_video::codec::RenderGpu::Nvidia
    } else {
        livi_video::codec::RenderGpu::Node(name)
    })
}

#[cfg(target_os = "android")]
fn media_codecs(codec: VideoCodec) -> Vec<String> {
    let Some(caps) = livi_video::codec::caps_for(codec)
        .to_str()
        .ok()
        .and_then(|caps| caps.parse::<gst::Caps>().ok())
    else {
        return Vec::new();
    };
    let mut decoders = gst::ElementFactory::factories_with_type(
        gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO,
        gst::Rank::MARGINAL,
    )
    .into_iter()
    .filter(|f| f.name().starts_with("amcviddec") && f.can_sink_any_caps(&caps))
    .collect::<Vec<_>>();
    decoders.sort_by_key(|f| std::cmp::Reverse(f.rank()));
    decoders.into_iter().map(|f| f.name().to_string()).collect()
}

fn candidates(codec: VideoCodec, sw_only: bool) -> Vec<String> {
    #[cfg(target_os = "android")]
    if !sw_only {
        let mut names = media_codecs(codec);
        names.push(
            livi_video::codec::sw_decoder_for(codec)
                .to_str()
                .unwrap_or("")
                .to_owned(),
        );
        return names;
    }
    match render_gpu() {
        Some(gpu) => livi_video::codec::decoder_candidates_on(codec, sw_only, &gpu),
        None => livi_video::codec::decoder_candidates(codec, sw_only)
            .iter()
            .filter_map(|name| name.to_str().ok())
            .map(str::to_owned)
            .collect(),
    }
}

fn usable(name: &str) -> bool {
    if gst::ElementFactory::find(name).is_none() {
        return false;
    }
    let Ok(node) = std::env::var("LIVI_RENDER_NODE") else {
        return true;
    };
    if !name.starts_with("va") || name.starts_with("varender") {
        return true;
    }
    let Ok(dec) = gst::ElementFactory::make(name).build() else {
        return false;
    };
    dec.find_property("device-path").is_none()
        || dec.property::<Option<String>>("device-path").as_deref() == Some(node.as_str())
}

pub fn reserve_scanout(handle: usize) {
    #[cfg(all(livi_os, not(in_process)))]
    view::reserve(handle);
    #[cfg(not(all(livi_os, not(in_process))))]
    let _ = handle;
}

pub fn set_curves(handle: usize, rgb: &[f32]) {
    #[cfg(all(livi_os, not(in_process)))]
    view::curves(handle, rgb);
    #[cfg(not(all(livi_os, not(in_process))))]
    let _ = (handle, rgb);
}

pub fn plays_urls() -> bool {
    ensure_init();
    gst::ElementFactory::find("playbin3").is_some()
}

pub fn probe(codec: VideoCodec) -> (bool, bool) {
    ensure_init();
    let exists = |name: &str| gst::ElementFactory::find(name).is_some();

    let best = candidates(codec, false)
        .into_iter()
        .find(|name| usable(name));

    let hw = best
        .as_deref()
        .is_some_and(livi_video::codec::is_hw_decoder);
    let sw = exists(
        livi_video::codec::sw_decoder_for(codec)
            .to_str()
            .unwrap_or(""),
    );
    (hw, sw)
}
