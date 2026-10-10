use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use livi_host_proto::UrlStatus;

/// Above every decoder decodebin3 would pick on its own.
const PICKED_RANK: i32 = 1024;

/// A stream playbin3 fetches, times and decodes itself, picture and sound.
pub struct UrlPlayer {
    playbin: gst::Element,
    heard: Arc<Mutex<Heard>>,
}

/// What only the bus tells.
#[derive(Default)]
struct Heard {
    buffered_percent: Option<u8>,
    ended: bool,
    failed: bool,
}

impl UrlPlayer {
    /// An empty `audio_device` is the system default.
    pub fn new(url: &str, audio_device: &str) -> Option<Self> {
        crate::ensure_init();
        prefer_our_decoders();
        let playbin = gst::ElementFactory::make("playbin3")
            .property("uri", url)
            .build()
            .inspect_err(|e| eprintln!("[gst_video] no playbin3: {e}"))
            .ok()?;
        // No subtitles and no deinterlacing, the bundle carries neither.
        playbin.set_property_from_str("flags", "video+audio+soft-volume");
        playbin.set_property("video-sink", video_sink()?);
        if let Some(sink) = audio_sink(audio_device) {
            playbin.set_property("audio-sink", sink);
        }
        playbin.connect("element-setup", false, |args| {
            if let Some(element) = args.get(1).and_then(|a| a.get::<gst::Element>().ok()) {
                watch_decoder(&element);
            }
            None
        });
        let heard = Arc::new(Mutex::new(Heard::default()));
        watch_bus(&playbin, heard.clone());
        eprintln!("[gst_video] playbin3 plays {url}");
        Some(Self { playbin, heard })
    }

    pub fn start(&self) {
        let _ = self.playbin.set_state(gst::State::Playing);
    }

    /// 0 pauses.
    pub fn set_rate(&self, rate: f64) {
        let state = if rate > 0.0 { gst::State::Playing } else { gst::State::Paused };
        let _ = self.playbin.set_state(state);
    }

    pub fn seek(&self, seconds: f64) {
        let to = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64);
        let flags = gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT;
        if let Err(e) = self.playbin.seek_simple(flags, to) {
            eprintln!("[gst_video] seek to {seconds:.1} s failed: {e}");
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.playbin.set_property("mute", muted);
    }

    pub fn status(&self) -> UrlStatus {
        let (_, state, _) = self.playbin.state(gst::ClockTime::ZERO);
        let heard = self.heard.lock().unwrap_or_else(|e| e.into_inner());
        UrlStatus {
            position: self.playbin.query_position::<gst::ClockTime>().map(seconds),
            duration: self.playbin.query_duration::<gst::ClockTime>().map(seconds),
            seekable: seekable(&self.playbin),
            playing: state == gst::State::Playing,
            ready: matches!(state, gst::State::Paused | gst::State::Playing),
            buffered_percent: heard.buffered_percent,
            ended: heard.ended,
            failed: heard.failed,
        }
    }
}

impl Drop for UrlPlayer {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

/// The decoder LIVI picks for its own planes, so the URL lands on the same GPU.
fn prefer_our_decoders() {
    let sw_only = std::env::var_os("LIVI_GST_SWDEC").is_some();
    for codec in ["h264", "h265"] {
        let picked = crate::candidates(codec, sw_only).into_iter().find(|n| crate::usable(n));
        if let Some(factory) = picked.and_then(|n| gst::ElementFactory::find(&n)) {
            factory.set_rank(gst::Rank::from(PICKED_RANK));
        }
    }
}

fn video_sink() -> Option<gst::Element> {
    let desc = std::env::var("LIVI_GST_SINK")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "waylandsink".to_owned());
    gst::parse::bin_from_description(&desc, true)
        .inspect_err(|e| eprintln!("[gst_video] no video sink {desc}: {e}"))
        .ok()
        .map(|bin| bin.upcast())
}

fn audio_sink(device: &str) -> Option<gst::Element> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let sink = gst::ElementFactory::make("pulsesink").build().ok()?;
    if !device.is_empty() {
        sink.set_property("device", device);
    }
    Some(sink)
}

fn watch_decoder(element: &gst::Element) {
    let Some(factory) = element.factory() else { return };
    if factory.has_type(gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO) {
        eprintln!("[gst_video] playbin3 decodes with {}", factory.name());
        crate::install_decoder_probes(element, factory.name().as_str());
    }
}

/// hlsdemux2 finds its way back into a live stream itself. Starting over would cost the window
/// the compositor bound to the plane.
fn watch_bus(playbin: &gst::Element, heard: Arc<Mutex<Heard>>) {
    let Some(bus) = playbin.bus() else { return };
    bus.set_sync_handler(move |_, msg| {
        let src = msg.src().map(|s| s.name().to_string()).unwrap_or_default();
        let mut heard = heard.lock().unwrap_or_else(|e| e.into_inner());
        match msg.view() {
            gst::MessageView::Buffering(b) => {
                heard.buffered_percent = u8::try_from(b.percent().clamp(0, 100)).ok();
            }
            gst::MessageView::Error(e) => {
                heard.failed = true;
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
            gst::MessageView::StreamCollection(c) => {
                for stream in c.stream_collection().iter() {
                    let caps = stream.caps();
                    let what = caps
                        .as_ref()
                        .and_then(|c| c.structure(0))
                        .map(|s| {
                            let size = s.get::<i32>("width").ok().zip(s.get::<i32>("height").ok());
                            let size = size.map(|(w, h)| format!(" {w}x{h}")).unwrap_or_default();
                            format!("{}{size}", s.name())
                        })
                        .unwrap_or_default();
                    eprintln!("[gst_video] playbin3 stream {:?} {what}", stream.stream_type());
                }
            }
            gst::MessageView::Eos(_) => {
                heard.ended = true;
                eprintln!("[gst_video] playbin3 reached the end");
            }
            _ => {}
        }
        gst::BusSyncReply::Drop
    });
}

fn seconds(t: gst::ClockTime) -> f64 {
    t.nseconds() as f64 / 1e9
}

fn seekable(playbin: &gst::Element) -> Option<(f64, f64)> {
    let mut query = gst::query::Seeking::new(gst::Format::Time);
    if !playbin.query(&mut query) {
        return None;
    }
    match query.result() {
        (
            true,
            gst::GenericFormattedValue::Time(Some(start)),
            gst::GenericFormattedValue::Time(Some(end)),
        ) => Some((seconds(start), seconds(end))),
        _ => None,
    }
}
