// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use livi_audio::stream::{AudioSink, Codec as AudioCodec};
use livi_audio::uplink::UplinkCodec;
pub use livi_host_proto::UrlStatus;
use livi_host_proto::{
    CLUSTER_PLANE_MAX, CLUSTER_PLANE_MIN, CLUSTER_RECV_ID, Framer, cameras_body, feed as feedproto,
    feeder_of, parse_camera_body, parse_plane_body, parse_url_body, url_status_body,
};
pub use livi_host_proto::{CameraOrder, CameraSeen};
use livi_screen_stream::ScreenSink;
use livi_video::fanout::Fanout;
use livi_video::nal::CpCodec;

mod audio;
pub mod feed;
pub mod gst;
pub mod process;
mod spectrum;
pub mod ui_plane;
mod video;

use livi_host_proto::ops::*;
use spectrum::Spectrum;

const DELAY_STEP_MS: u32 = 10;
const DELAY_WEIGHT: f64 = 0.2;

pub trait Speaker: Send + Sync + 'static {
    fn push_rtp(&self, rtp: &[u8]);
    fn push_samples(&self, samples: &[u8]);
    fn set_volume(&self, level: f64, ms: u64);
    fn set_visualizer_enabled(&self, on: bool);
    fn take_visualizer(&self) -> Option<(Vec<u8>, u32)>;
    fn output_delay_ms(&self) -> Option<u32>;
}

pub struct AudioConfig {
    pub codec: AudioCodec,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub channels: u8,
    pub latency_ms: u32,
    pub realtime: bool,
    pub device: Option<String>,
    pub key: [u8; 32],
    pub codec_data: Vec<u8>,
}

pub struct UplinkConfig {
    pub codec: UplinkCodec,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub frame_ms: u32,
    pub port: u16,
    pub key: [u8; 32],
    pub phone: String,
    pub device: Option<String>,
}

pub struct TapConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub device: Option<String>,
    pub path: String,
}

pub trait Plane: 'static {
    fn start(&self);
    fn push(&self, nal: &[u8]);
    fn flush(&self);
    fn set_gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64);
}

pub trait UrlPlay: 'static {
    fn start(&self);
    fn set_rate(&self, rate: f64);
    fn seek(&self, seconds: f64);
    fn set_muted(&self, muted: bool);
    fn status(&self) -> UrlStatus;
}

pub trait Outside: 'static {
    type Plane: Plane;
    type UrlPlayer: UrlPlay;
    type Ears;

    type Speaker: Speaker;
    type AudioEars;

    fn create_plane(&self, id: u32, codec: &str, codec_data: &[u8]) -> Option<Self::Plane>;

    fn play_url(&self, id: u32, url: &str, audio_device: &str) -> Option<Self::UrlPlayer>;

    type Camera;

    fn open_camera(&self, id: u32, order: &CameraOrder) -> Option<Self::Camera>;

    fn watch_cameras(&self, report: Box<dyn Fn(Vec<CameraSeen>) + Send>);

    fn listen(&self, key: [u8; 32], sink: Box<dyn ScreenSink>) -> Option<(Self::Ears, u16)>;

    type Uplink;

    fn create_speaker(&self, cfg: &AudioConfig) -> Option<Self::Speaker>;

    fn open_uplink(&self, cfg: UplinkConfig) -> Option<Self::Uplink>;
    type Tap;
    fn open_tap(&self, cfg: TapConfig) -> Option<Self::Tap>;

    fn listen_audio(
        &self,
        key: [u8; 32],
        sink: Box<dyn AudioSink + Send>,
    ) -> Option<(Self::AudioEars, u16, u16)>;

    type FeedEars;

    fn open_feed(&self, path: &str, sink: Box<dyn MediaSink>) -> Option<Self::FeedEars>;
}

pub trait MediaSink {
    fn on_record(&mut self, record: feedproto::Record);
}

pub trait Wire: Send + Sync {
    fn reply(&self, op: u8, id: u32, rest: &[u8]);
}

type Planes<P> = Rc<RefCell<HashMap<u32, P>>>;

struct ReceiverState {
    plane_id: u32,
    is_cluster: bool,
    config: Vec<u8>,
    fan: Fanout,
}

impl ReceiverState {
    fn for_each_target<P: Plane>(&self, planes: &HashMap<u32, P>, mut f: impl FnMut(&P)) {
        if self.is_cluster {
            for id in CLUSTER_PLANE_MIN..=CLUSTER_PLANE_MAX {
                if let Some(p) = planes.get(&id) {
                    f(p);
                }
            }
        } else if let Some(p) = planes.get(&self.plane_id) {
            f(p);
        }
    }

    fn has_target<P: Plane>(&self, planes: &HashMap<u32, P>) -> bool {
        let mut any = false;
        self.for_each_target(planes, |_| any = true);
        any
    }
}

struct Feed<O: Outside> {
    state: Rc<RefCell<ReceiverState>>,
    planes: Planes<O::Plane>,
    wire: Arc<dyn Wire>,
}

impl<O: Outside> ScreenSink for Feed<O> {
    fn on_config(&mut self, codec: CpCodec, atom: &[u8]) {
        let mut st = self.state.borrow_mut();
        st.fan.set_codec(codec);
        if atom.is_empty() {
            return;
        }
        st.config.clear();
        st.config.push(codec as u8);
        st.config.extend_from_slice(atom);
        if st.fan.is_active() {
            self.wire.reply(REPLY_CONFIG, st.plane_id, &st.config);
        }
    }

    fn on_frame(&mut self, nal: &[u8]) {
        let planes = self.planes.borrow();
        let mut st = self.state.borrow_mut();
        let has_target = st.has_target(&planes);
        if st.fan.take(nal, has_target) {
            st.for_each_target(&planes, |p| p.push(nal));
        }
    }

    fn on_started(&mut self) {
        let st = self.state.borrow();
        if st.fan.is_active() {
            self.wire.reply(REPLY_STARTED, st.plane_id, &[]);
        }
    }
}

struct Receiver<E> {
    state: Rc<RefCell<ReceiverState>>,
    _ears: E,
}

struct Output<S> {
    now: std::sync::Mutex<Arc<S>>,
}

impl<S> Output<S> {
    fn new(speaker: S) -> Arc<Self> {
        Arc::new(Self {
            now: std::sync::Mutex::new(Arc::new(speaker)),
        })
    }

    fn get(&self) -> Arc<S> {
        self.now.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn replace(&self, speaker: S) {
        let old = std::mem::replace(
            &mut *self.now.lock().unwrap_or_else(|e| e.into_inner()),
            Arc::new(speaker),
        );
        drop(old);
    }
}

struct AudioFeed<O: Outside> {
    speaker: Arc<Output<O::Speaker>>,
    wire: Arc<dyn Wire>,
    id: u32,
    active: Arc<AtomicBool>,
}

impl<O: Outside> AudioSink for AudioFeed<O> {
    fn on_started(&mut self, first_sample: u32) {
        self.wire
            .reply(REPLY_AUDIO_STARTED, self.id, &first_sample.to_le_bytes());
    }

    fn on_rtp(&mut self, rtp: &[u8], _sample: u32) {
        if self.active.load(Ordering::Relaxed) {
            self.speaker.get().push_rtp(rtp);
        }
    }
}

struct AudioStream<S, E> {
    speaker: Arc<Output<S>>,
    cfg: AudioConfig,
    level: Option<f64>,
    active: Arc<AtomicBool>,
    told_delay_ms: Option<u32>,
    smoothed_delay_ms: Option<f64>,
    _ears: Option<E>,
}

type Streams<S, E> = Rc<RefCell<HashMap<u32, AudioStream<S, E>>>>;
type FeedFans = Rc<RefCell<HashMap<u32, Option<Fanout>>>>;
type FeedWanted = Rc<RefCell<HashMap<u32, bool>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Plane(u32),
    Audio(u32),
}

type FeedRoutes = Rc<RefCell<HashMap<u32, Route>>>;

struct MediaFeed<O: Outside> {
    planes: Planes<O::Plane>,
    audio: Streams<O::Speaker, O::AudioEars>,
    fans: FeedFans,
    wanted: FeedWanted,
    routes: FeedRoutes,
}

impl<O: Outside> MediaFeed<O> {
    fn for_each_target(planes: &HashMap<u32, O::Plane>, id: u32, mut f: impl FnMut(&O::Plane)) {
        if id == CLUSTER_RECV_ID {
            for cid in CLUSTER_PLANE_MIN..=CLUSTER_PLANE_MAX {
                if let Some(p) = planes.get(&cid) {
                    f(p);
                }
            }
        } else if let Some(p) = planes.get(&id) {
            f(p);
        }
    }
}

impl<O: Outside> MediaSink for MediaFeed<O> {
    fn on_record(&mut self, r: feedproto::Record) {
        match r.kind {
            feedproto::KIND_VIDEO_START => {
                let fan = match r.payload.first() {
                    Some(0) => Some(CpCodec::H264),
                    Some(1) => Some(CpCodec::H265),
                    _ => None,
                }
                .map(|codec| {
                    let mut fan = Fanout::new();
                    fan.set_codec(codec);
                    fan.set_active(self.wanted.borrow().get(&r.id).copied().unwrap_or(true));
                    fan
                });
                self.fans.borrow_mut().insert(r.id, fan);
            }
            feedproto::KIND_VIDEO => {
                let target = match self.routes.borrow().get(&r.id) {
                    Some(Route::Plane(plane)) => *plane,
                    _ => r.id,
                };
                let planes = self.planes.borrow();
                let mut has_target = false;
                Self::for_each_target(&planes, target, |_| has_target = true);
                let pass = match self.fans.borrow_mut().get_mut(&r.id) {
                    Some(Some(fan)) => fan.take(&r.payload, has_target),
                    _ => has_target,
                };
                if pass {
                    Self::for_each_target(&planes, target, |p| p.push(&r.payload));
                }
            }
            feedproto::KIND_AUDIO => {
                let (stream, wanted) = match self.routes.borrow().get(&r.id) {
                    Some(Route::Audio(stream)) => (
                        *stream,
                        self.wanted.borrow().get(&r.id).copied().unwrap_or(false),
                    ),
                    _ => (r.id, true),
                };
                if wanted
                    && let Some(a) = self.audio.borrow().get(&stream)
                    && a.active.load(Ordering::Relaxed)
                {
                    a.speaker.get().push_samples(&r.payload);
                }
            }
            _ => {}
        }
    }
}

pub struct Host<O: Outside> {
    outside: O,
    framer: Framer,
    planes: Planes<O::Plane>,
    receivers: HashMap<u32, Receiver<O::Ears>>,
    audio: Streams<O::Speaker, O::AudioEars>,
    uplinks: HashMap<u32, O::Uplink>,
    taps: HashMap<u32, O::Tap>,
    url_players: HashMap<u32, O::UrlPlayer>,
    cameras: HashMap<u32, O::Camera>,
    cameras_watched: bool,
    feed_fans: FeedFans,
    feed_wanted: FeedWanted,
    feed_routes: FeedRoutes,
    helper_feed: Option<O::FeedEars>,
    visualizer_enabled: bool,
    spectrum: Spectrum,
    wire: Arc<dyn Wire>,
}

impl<O: Outside> Host<O> {
    pub fn new(outside: O, wire: Arc<dyn Wire>) -> Self {
        Self {
            outside,
            framer: Framer::new(),
            planes: Rc::new(RefCell::new(HashMap::new())),
            receivers: HashMap::new(),
            audio: Rc::new(RefCell::new(HashMap::new())),
            uplinks: HashMap::new(),
            taps: HashMap::new(),
            url_players: HashMap::new(),
            cameras: HashMap::new(),
            cameras_watched: false,
            feed_fans: Rc::new(RefCell::new(HashMap::new())),
            feed_wanted: Rc::new(RefCell::new(HashMap::new())),
            feed_routes: Rc::new(RefCell::new(HashMap::new())),
            helper_feed: None,
            visualizer_enabled: false,
            spectrum: Spectrum::new(),
            wire,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        self.framer.push(chunk);
        while let Some(m) = self.framer.next_message() {
            self.dispatch(m.op, m.id, &m.rest);
        }
    }

    fn dispatch(&mut self, op: u8, id: u32, rest: &[u8]) {
        match op {
            OP_CREATE => self.create_plane(id, rest),
            OP_DATA => {
                if let Some(p) = self.planes.borrow().get(&id) {
                    p.push(rest);
                }
            }
            OP_STOP => {
                self.planes.borrow_mut().remove(&id);
                self.cameras.remove(&id);
                if self.url_players.remove(&id).is_some() {
                    eprintln!(
                        "{}",
                        livi_video_player::stamped("[gst_video] player dropped with all it held")
                    );
                }
            }
            OP_PLAY_URL => self.play_url(id, rest),
            OP_CAMERA_OPEN => self.open_camera(id, rest),
            OP_CAMERAS_WATCH => self.watch_cameras(),
            OP_PLAY_RATE => {
                if let (Some(p), Some(rate)) = (self.url_players.get(&id), f64_le(rest)) {
                    p.set_rate(rate);
                }
            }
            OP_PLAY_SEEK => {
                if let (Some(p), Some(seconds)) = (self.url_players.get(&id), f64_le(rest)) {
                    p.seek(seconds);
                }
            }
            OP_PLAY_MUTE => {
                if let (Some(p), Some(b)) = (self.url_players.get(&id), rest.first()) {
                    p.set_muted(b & 1 != 0);
                }
            }
            OP_GAMMA => self.set_gamma(id, rest),
            OP_LISTEN => self.open_receiver(id, rest),
            OP_TEARDOWN => {
                self.receivers.remove(&id);
            }
            OP_SET_ACTIVE => self.set_active_feeder(id, rest),
            OP_AUDIO_OPEN => self.open_audio(id, rest),
            OP_AUDIO_VOLUME => {
                if rest.len() >= size_of::<f64>()
                    && let Some(a) = self.audio.borrow_mut().get_mut(&id)
                {
                    let level = f64::from_le_bytes(rest[..8].try_into().unwrap());
                    let ms = if rest.len() >= 12 {
                        u32::from_le_bytes(rest[8..12].try_into().unwrap()).into()
                    } else {
                        0
                    };
                    a.level = Some(level);
                    a.speaker.get().set_volume(level, ms);
                }
            }
            OP_AUDIO_OUTPUT => self.set_audio_output(rest),
            OP_AUDIO_STOP => {
                self.audio.borrow_mut().remove(&id);
            }
            OP_MIC_OPEN => self.open_uplink(id, rest),
            OP_MIC_STOP => {
                self.uplinks.remove(&id);
            }
            OP_TAP_OPEN => self.open_tap(id, rest),
            OP_TAP_STOP => {
                self.taps.remove(&id);
            }
            OP_AUDIO_DATA => {
                if let Some(a) = self.audio.borrow().get(&id)
                    && a.active.load(Ordering::Relaxed)
                {
                    a.speaker.get().push_samples(rest);
                }
            }
            OP_AUDIO_ACTIVE => {
                if let Some(a) = self.audio.borrow().get(&id) {
                    a.active
                        .store(rest.first().is_some_and(|b| b & 1 != 0), Ordering::Relaxed);
                }
            }
            OP_VISUALIZER => self.set_visualizer_enabled(rest.first().is_some_and(|b| b & 1 != 0)),
            OP_FEED_OPEN => self.open_feed(id, rest),
            OP_FEED_ROUTE => self.route_feed(id, rest),
            OP_FEED_CLOSE => {
                self.feed_routes.borrow_mut().remove(&id);
                self.feed_fans.borrow_mut().remove(&id);
                self.feed_wanted.borrow_mut().remove(&id);
            }
            _ => {}
        }
    }

    pub fn take_stats(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for r in self.receivers.values() {
            let mut st = r.state.borrow_mut();
            let awaiting = u8::from(st.fan.awaiting_keyframe());
            let active = u8::from(st.fan.is_active());
            let s = st.fan.take_stats();
            if s.incoming == 0 && s.dropped == 0 && s.pushed == 0 {
                continue;
            }
            lines.push(format!(
                "[cp_screen] recv 0x{:x}: in={} dropped={} pushed={} awaiting_kf={awaiting} active={active}",
                st.plane_id, s.incoming, s.dropped, s.pushed
            ));
        }
        let routes = self.feed_routes.borrow();
        for (id, fan) in self.feed_fans.borrow_mut().iter_mut() {
            let Some(fan) = fan else { continue };
            let awaiting = u8::from(fan.awaiting_keyframe());
            let active = u8::from(fan.is_active());
            let s = fan.take_stats();
            if s.incoming == 0 && s.dropped == 0 && s.pushed == 0 {
                continue;
            }
            let to = match routes.get(id) {
                Some(Route::Plane(plane)) => format!(" -> 0x{plane:x}"),
                _ => String::new(),
            };
            lines.push(format!(
                "[feed] recv 0x{id:x}{to}: in={} dropped={} pushed={} awaiting_kf={awaiting} active={active}",
                s.incoming, s.dropped, s.pushed
            ));
        }
        lines
    }
}

fn f64_le(rest: &[u8]) -> Option<f64> {
    Some(f64::from_le_bytes(rest.get(..8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests;
