// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use livi_host_proto::UrlStatus;

use crate::Say;

const PICKED_RANK: i32 = 1024;

pub struct UrlPlayer {
    playbin: gst::Element,
    heard: Arc<Mutex<Heard>>,
    view: crate::WindowView,
    say: Say,
}

#[derive(Default)]
struct Heard {
    buffered_percent: Option<u8>,
    ended: bool,
    failed: bool,
    hold: Hold,
    start_at: Option<f64>,
}

#[derive(Debug, Default)]
struct Hold {
    wanted: bool,
    filling: bool,
}

impl Hold {
    fn state(&self) -> gst::State {
        if self.wanted && !self.filling {
            gst::State::Playing
        } else {
            gst::State::Paused
        }
    }

    fn rejoins(&mut self, playing: bool, live: bool) -> bool {
        let rejoin = playing && !self.wanted && live;
        if rejoin {
            self.filling = false;
        }
        rejoin
    }

    fn buffering(&mut self, percent: i32) -> Option<gst::State> {
        let filling = percent < 100;
        if filling == self.filling {
            return None;
        }
        self.filling = filling;
        self.wanted.then(|| self.state())
    }
}

impl UrlPlayer {
    pub fn new(url: &str, audio_device: &str, window: usize, say: Say) -> Option<Self> {
        crate::ensure_init();
        prefer_our_decoders();
        let playbin = gst::ElementFactory::make("playbin3")
            .property("uri", url)
            .build()
            .inspect_err(|e| say(&format!("[gst_video] no playbin3: {e}")))
            .ok()?;
        playbin.set_property_from_str("flags", "video+audio+soft-volume+buffering");
        let video = video_sink(&say)?;
        let view = crate::WindowView::default();
        view.attach(window, crate::RANK_URL, Some(&video));
        let heard = Arc::new(Mutex::new(Heard::default()));
        playbin.set_property("video-sink", video);
        if let Some(sink) = audio_sink(audio_device) {
            playbin.set_property("audio-sink", sink);
        }
        let decoders = say.clone();
        playbin.connect("element-setup", false, move |args| {
            if let Some(element) = args.get(1).and_then(|a| a.get::<gst::Element>().ok()) {
                watch_decoder(&element, &decoders);
            }
            None
        });
        watch_bus(&playbin, heard.clone(), say.clone());
        say(&format!("[gst_video] playbin3 plays {url}"));
        Some(Self {
            playbin,
            heard,
            view,
            say,
        })
    }

    pub fn start(&self) {
        (self.say)("[gst_video] start");
        self.want(true);
    }

    fn want(&self, playing: bool) {
        let state = {
            let mut heard = self.heard.lock().unwrap_or_else(|e| e.into_inner());
            heard.hold.wanted = playing;
            heard.hold.state()
        };
        let _ = self.playbin.set_state(state);
    }

    pub fn stop(&self) {
        (self.say)("[gst_video] stop, everything it held goes");
        let _ = self.playbin.set_state(gst::State::Null);
        self.view.remove();
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

    pub fn set_rate(&self, rate: f64) {
        (self.say)(&format!("[gst_video] the phone sets the rate to {rate}"));
        let live = self.playbin.query_duration::<gst::ClockTime>().is_none();
        let rejoin = {
            let mut heard = self.heard.lock().unwrap_or_else(|e| e.into_inner());
            let rejoin = heard.hold.rejoins(rate > 0.0, live);
            if rejoin {
                heard.buffered_percent = None;
            }
            rejoin
        };
        if rejoin {
            (self.say)("[gst_video] live stream resumes, everything held goes, joining it anew");
            let _ = self.playbin.set_state(gst::State::Ready);
        }
        self.want(rate > 0.0);
    }

    pub fn seek(&self, seconds: f64) {
        (self.say)(&format!("[gst_video] the phone seeks to {seconds:.3} s"));
        {
            let mut heard = self.heard.lock().unwrap_or_else(|e| e.into_inner());
            let (_, state, _) = self.playbin.state(gst::ClockTime::ZERO);
            if !matches!(state, gst::State::Paused | gst::State::Playing) {
                heard.start_at = Some(seconds);
                return;
            }
        }
        seek_to(&self.playbin, seconds, &self.say);
    }

    pub fn set_muted(&self, muted: bool) {
        (self.say)(&format!("[gst_video] the phone sets muted {muted}"));
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
        self.view.remove();
    }
}

fn prefer_our_decoders() {
    let sw_only = std::env::var_os("LIVI_GST_SWDEC").is_some();
    for codec in ["h264", "h265"] {
        let picked = crate::candidates(codec, sw_only)
            .into_iter()
            .find(|n| crate::usable(n));
        if let Some(factory) = picked.and_then(|n| gst::ElementFactory::find(&n)) {
            factory.set_rank(gst::Rank::from(PICKED_RANK));
        }
    }
}

pub(crate) fn video_sink(say: &Say) -> Option<gst::Element> {
    if cfg!(target_os = "macos") {
        return gst::ElementFactory::make("glimagesink")
            .build()
            .inspect_err(|e| say(&format!("[gst_video] no glimagesink: {e}")))
            .ok();
    }
    let desc = std::env::var("LIVI_GST_SINK")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "waylandsink".to_owned());
    gst::parse::bin_from_description(&desc, true)
        .inspect_err(|e| say(&format!("[gst_video] no video sink {desc}: {e}")))
        .ok()
        .map(|bin| bin.upcast())
}

fn audio_sink(device: &str) -> Option<gst::Element> {
    let (element, device_property) = if cfg!(target_os = "macos") {
        ("osxaudiosink", "unique-id")
    } else {
        ("pulsesink", "device")
    };
    let sink = gst::ElementFactory::make(element).build().ok()?;
    if !device.is_empty() {
        sink.set_property(device_property, device);
    }
    Some(sink)
}

fn watch_decoder(element: &gst::Element, say: &Say) {
    let Some(factory) = element.factory() else {
        return;
    };
    if factory.has_type(gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO) {
        say(&format!(
            "[gst_video] playbin3 decodes with {}",
            factory.name()
        ));
        crate::install_decoder_probes(element, factory.name().as_str(), say);
    }
}

fn watch_bus(playbin: &gst::Element, heard: Arc<Mutex<Heard>>, say: Say) {
    let Some(bus) = playbin.bus() else { return };
    let player = playbin.downgrade();
    let own_name = playbin.name().to_string();
    bus.set_sync_handler(move |_, msg| {
        let src = msg.src().map(|s| s.name().to_string()).unwrap_or_default();
        let mut heard = heard.lock().unwrap_or_else(|e| e.into_inner());
        match msg.view() {
            gst::MessageView::StateChanged(s) if src == own_name => {
                say(&format!(
                    "[gst_video] playbin3 {:?} to {:?}",
                    s.old(),
                    s.current()
                ));
                let prerolled = s.old() == gst::State::Ready && s.current() == gst::State::Paused;
                if prerolled
                    && let Some(at) = heard.start_at.take()
                    && let Some(playbin) = player.upgrade()
                {
                    let say = say.clone();
                    playbin.call_async(move |p| {
                        if p.query_duration::<gst::ClockTime>().is_none() {
                            say(&format!("[gst_video] a live stream, {at:.3} s is left out"));
                            return;
                        }
                        say(&format!(
                            "[gst_video] the stream is ready, seeking to {at:.3} s"
                        ));
                        seek_to(p, at, &say);
                    });
                }
            }
            gst::MessageView::Buffering(b) => {
                let percent = b.percent().clamp(0, 100);
                heard.buffered_percent = u8::try_from(percent).ok();
                if let Some(state) = heard.hold.buffering(percent) {
                    say(&format!(
                        "[gst_video] buffered {percent} %, switching to {state:?}"
                    ));
                    if let Some(playbin) = player.upgrade() {
                        playbin.call_async(move |p| {
                            let _ = p.set_state(state);
                        });
                    }
                }
            }
            gst::MessageView::Error(e) => {
                heard.failed = true;
                say(&format!(
                    "[gst_video] ERROR from {src}: {} | {}",
                    e.error(),
                    e.debug().unwrap_or_default()
                ));
            }
            gst::MessageView::Warning(w) => {
                say(&format!(
                    "[gst_video] WARN from {src}: {} | {}",
                    w.error(),
                    w.debug().unwrap_or_default()
                ));
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
                    say(&format!(
                        "[gst_video] playbin3 stream {:?} {what}",
                        stream.stream_type()
                    ));
                }
            }
            gst::MessageView::Eos(_) => {
                heard.ended = true;
                say("[gst_video] playbin3 reached the end");
            }
            _ => {}
        }
        gst::BusSyncReply::Drop
    });
}

fn seek_to(playbin: &gst::Element, seconds: f64, say: &Say) {
    let to = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1e9) as u64);
    let flags = gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT;
    if let Err(e) = playbin.seek_simple(flags, to) {
        say(&format!("[gst_video] seek to {seconds:.1} s failed: {e}"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_waits_for_a_full_queue_and_only_while_the_phone_wants_it() {
        let mut hold = Hold::default();
        assert_eq!(hold.state(), gst::State::Paused);
        hold.wanted = true;
        assert_eq!(hold.state(), gst::State::Playing);

        assert_eq!(hold.buffering(10), Some(gst::State::Paused));
        assert_eq!(hold.buffering(60), None, "only a change switches");
        assert_eq!(hold.state(), gst::State::Paused);
        assert_eq!(hold.buffering(100), Some(gst::State::Playing));
        assert_eq!(hold.buffering(100), None);

        hold.wanted = false;
        assert_eq!(
            hold.buffering(0),
            None,
            "paused by the phone, nothing to hold"
        );
        assert_eq!(hold.buffering(100), None);
        assert_eq!(hold.state(), gst::State::Paused);

        hold.buffering(30);
        hold.wanted = true;
        assert_eq!(
            hold.state(),
            gst::State::Paused,
            "a play while filling waits for the queue"
        );
        assert_eq!(hold.buffering(100), Some(gst::State::Playing));
    }

    #[test]
    fn only_a_live_stream_resumed_after_a_pause_is_joined_anew() {
        let mut hold = Hold {
            wanted: true,
            filling: false,
        };
        assert!(
            !hold.rejoins(true, true),
            "already playing, nothing ran away"
        );

        hold.wanted = false;
        assert!(!hold.rejoins(false, true), "still paused");
        assert!(
            !hold.rejoins(true, false),
            "a stream with an end waits where it was"
        );

        hold.buffering(20);
        assert!(hold.rejoins(true, true));
        hold.wanted = true;
        assert_eq!(
            hold.state(),
            gst::State::Playing,
            "the old queue no longer holds it back"
        );
    }
}
