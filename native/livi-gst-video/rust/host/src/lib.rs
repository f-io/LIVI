use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use livi_audio_stream::{AudioSink, Codec as AudioCodec};
use livi_audio_uplink::UplinkCodec;
pub use livi_host_proto::UrlStatus;
use livi_host_proto::{
    CLUSTER_PLANE_MAX, CLUSTER_PLANE_MIN, CLUSTER_RECV_ID, Framer, feed as feedproto, feeder_of,
    parse_plane_body, parse_url_body, url_status_body,
};
use livi_screen_stream::ScreenSink;
use livi_video_fanout::Fanout;
use livi_video_nal::CpCodec;

pub mod feed;
pub mod gst;
pub mod process;
mod spectrum;
pub mod ui_plane;

use spectrum::Spectrum;

const OP_CREATE: u8 = 1;
const OP_DATA: u8 = 2;
const OP_STOP: u8 = 3;
const OP_GAMMA: u8 = 4;
const OP_LISTEN: u8 = 5;
const OP_TEARDOWN: u8 = 6;
const OP_SET_ACTIVE: u8 = 7;
const OP_AUDIO_OPEN: u8 = 8;
const OP_AUDIO_VOLUME: u8 = 9;
const OP_AUDIO_STOP: u8 = 10;
const OP_MIC_OPEN: u8 = 11;
const OP_MIC_STOP: u8 = 12;
const OP_TAP_OPEN: u8 = 20;
const OP_TAP_STOP: u8 = 21;
const OP_AUDIO_ACTIVE: u8 = 13;
const OP_AUDIO_DATA: u8 = 14;
const OP_VISUALIZER: u8 = 15;
const OP_FEED_OPEN: u8 = 16;
const OP_AUDIO_OUTPUT: u8 = 17;
const OP_FEED_ROUTE: u8 = 18;
const OP_FEED_CLOSE: u8 = 19;
const OP_PLAY_URL: u8 = 22;
const OP_PLAY_RATE: u8 = 23;
const OP_PLAY_SEEK: u8 = 24;
const OP_PLAY_MUTE: u8 = 25;

const REPLY_PORT: u8 = 1;
const REPLY_CONFIG: u8 = 2;
const REPLY_STARTED: u8 = 3;
const REPLY_AUDIO_PORTS: u8 = 4;
const REPLY_AUDIO_STARTED: u8 = 5;
const REPLY_VISUALIZER: u8 = 6;
const REPLY_FEED: u8 = 7;
const REPLY_URL_STATUS: u8 = 8;

pub trait Speaker: Send + Sync + 'static {
    fn push_rtp(&self, rtp: &[u8]);
    fn push_samples(&self, samples: &[u8]);
    fn set_volume(&self, level: f64, ms: u64);
    /// The tap is pre-fader.
    fn set_visualizer_enabled(&self, on: bool);
    /// Mono samples and their rate.
    fn take_visualizer(&self) -> Option<(Vec<u8>, u32)>;
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

/// A plane that fetches its stream from a URL itself.
pub trait UrlPlay: 'static {
    fn start(&self);
    /// 0 pauses.
    fn set_rate(&self, rate: f64);
    fn seek(&self, seconds: f64);
    fn set_muted(&self, muted: bool);
    fn status(&self) -> UrlStatus;
}

pub trait Outside: 'static {
    type Plane: Plane;
    type UrlPlayer: UrlPlay;
    /// Dropping it stops the listening.
    type Ears;

    type Speaker: Speaker;
    /// Dropping them stops the listening.
    type AudioEars;

    fn create_plane(&self, id: u32, codec: &str, codec_data: &[u8]) -> Option<Self::Plane>;

    /// An empty `audio_device` is the system default.
    fn play_url(&self, id: u32, url: &str, audio_device: &str) -> Option<Self::UrlPlayer>;

    /// The port is the one to tell the phone about.
    fn listen(&self, key: [u8; 32], sink: Box<dyn ScreenSink>) -> Option<(Self::Ears, u16)>;

    /// Captures for as long as it is held.
    type Uplink;

    fn create_speaker(&self, cfg: &AudioConfig) -> Option<Self::Speaker>;

    fn open_uplink(&self, cfg: UplinkConfig) -> Option<Self::Uplink>;
    /// Captures for as long as it is held.
    type Tap;
    fn open_tap(&self, cfg: TapConfig) -> Option<Self::Tap>;

    /// The data port, then the control port.
    fn listen_audio(
        &self,
        key: [u8; 32],
        sink: Box<dyn AudioSink + Send>,
    ) -> Option<(Self::AudioEars, u16, u16)>;

    /// Dropping it stops the listening.
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
    /// The codec byte, then the configuration atom.
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
        // A keepalive config carries no record.
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
        Arc::new(Self { now: std::sync::Mutex::new(Arc::new(speaker)) })
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
    /// One phone at a time reaches the sink.
    active: Arc<AtomicBool>,
}

impl<O: Outside> AudioSink for AudioFeed<O> {
    fn on_started(&mut self, first_sample: u32) {
        self.wire.reply(REPLY_AUDIO_STARTED, self.id, &first_sample.to_le_bytes());
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
    /// Absent while the main process feeds the stream itself.
    _ears: Option<E>,
}

type Streams<S, E> = Rc<RefCell<HashMap<u32, AudioStream<S, E>>>>;
/// None for a codec the gate cannot read.
type FeedFans = Rc<RefCell<HashMap<u32, Option<Fanout>>>>;
type FeedWanted = Rc<RefCell<HashMap<u32, bool>>>;

/// Where a fed stream of its own ends. One phone session owns it, and it plays only while
/// that session is active.
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
                    Some(Route::Audio(stream)) => {
                        (*stream, self.wanted.borrow().get(&r.id).copied().unwrap_or(false))
                    }
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
                self.url_players.remove(&id);
            }
            OP_PLAY_URL => self.play_url(id, rest),
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
            // [8B level][4B ramp in ms, 0 for at once]
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
                    a.active.store(rest.first().is_some_and(|b| b & 1 != 0), Ordering::Relaxed);
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

    /// `[1B codecLen][codec ascii][codec_data]`
    fn create_plane(&mut self, id: u32, rest: &[u8]) {
        let Some((codec, codec_data)) = parse_plane_body(rest) else { return };

        self.planes.borrow_mut().remove(&id);
        self.url_players.remove(&id);
        let Some(plane) = self.outside.create_plane(id, &codec, codec_data) else {
            eprintln!("livi: create player 0x{id:x} (codec {codec}) FAILED");
            return;
        };
        plane.start();

        let feeder = feeder_of(id);
        if let Some(state) = self.active_feeder(feeder) {
            for frame in state.borrow().fan.primer() {
                plane.push(frame);
            }
        }
        if let Some(Some(fan)) = self.feed_fans.borrow().get(&feeder) {
            for frame in fan.primer() {
                plane.push(frame);
            }
        }
        for feed in self.feeds_to(Route::Plane(feeder), None) {
            if let Some(Some(fan)) = self.feed_fans.borrow().get(&feed)
                && fan.is_active()
            {
                for frame in fan.primer() {
                    plane.push(frame);
                }
            }
        }
        self.planes.borrow_mut().insert(id, plane);
    }

    /// `[2B deviceLen LE][audio device][url]`
    fn play_url(&mut self, id: u32, rest: &[u8]) {
        let Some((url, device)) = parse_url_body(rest) else { return };

        self.planes.borrow_mut().remove(&id);
        self.url_players.remove(&id);
        let Some(player) = self.outside.play_url(id, &url, &device) else {
            eprintln!("livi: play 0x{id:x} from {url} FAILED");
            return;
        };
        player.start();
        self.url_players.insert(id, player);
    }

    /// The socket path as utf-8. The reply carries the path back, empty when binding failed.
    fn open_feed(&mut self, id: u32, rest: &[u8]) {
        let Ok(path) = core::str::from_utf8(rest) else {
            return;
        };
        self.helper_feed = None;
        let sink = MediaFeed::<O> {
            planes: self.planes.clone(),
            audio: self.audio.clone(),
            fans: self.feed_fans.clone(),
            wanted: self.feed_wanted.clone(),
            routes: self.feed_routes.clone(),
        };
        match self.outside.open_feed(path, Box::new(sink)) {
            Some(ears) => {
                self.helper_feed = Some(ears);
                self.wire.reply(REPLY_FEED, id, rest);
            }
            None => self.wire.reply(REPLY_FEED, id, &[]),
        }
    }

    fn active_feeder(&self, plane_id: u32) -> Option<&Rc<RefCell<ReceiverState>>> {
        self.receivers.values().map(|r| &r.state).find(|state| {
            let st = state.borrow();
            st.fan.is_active() && st.plane_id == plane_id
        })
    }

    /// Five doubles: gamma, contrast and the three gains.
    fn set_gamma(&mut self, id: u32, rest: &[u8]) {
        const N: usize = 5;
        if rest.len() < N * size_of::<f64>() {
            return;
        }
        let mut v = [0f64; N];
        for (i, slot) in v.iter_mut().enumerate() {
            let bytes = &rest[i * size_of::<f64>()..][..size_of::<f64>()];
            *slot = f64::from_ne_bytes(bytes.try_into().unwrap());
        }
        if let Some(p) = self.planes.borrow().get(&id) {
            p.set_gamma(v[0], v[1], v[2], v[3], v[4]);
        }
    }

    /// `[4B planeId][1B flags: bit0=cluster][32B key]`. The message id names
    /// the receiver, one per session and screen.
    fn open_receiver(&mut self, id: u32, rest: &[u8]) {
        const FLAGS: usize = 5;
        const KEY_LEN: usize = 32;
        if rest.len() < FLAGS + KEY_LEN {
            return;
        }

        let state = Rc::new(RefCell::new(ReceiverState {
            plane_id: u32::from_ne_bytes(rest[..4].try_into().unwrap()),
            is_cluster: rest[4] & 1 != 0,
            config: Vec::new(),
            fan: Fanout::new(),
        }));
        let feed = Feed::<O> {
            state: state.clone(),
            planes: self.planes.clone(),
            wire: self.wire.clone(),
        };
        let key: [u8; KEY_LEN] = rest[FLAGS..FLAGS + KEY_LEN].try_into().unwrap();
        let Some((ears, port)) = self.outside.listen(key, Box::new(feed)) else {
            return;
        };

        self.receivers.insert(id, Receiver { state, _ears: ears });
        self.wire.reply(REPLY_PORT, id, &port.to_le_bytes());
    }

    /// `[1B kind: 0 plane, 1 audio stream][4B target][1B active]`. The message id names a fed
    /// stream of its own, which ends in the target.
    fn route_feed(&mut self, id: u32, rest: &[u8]) {
        if rest.len() < 6 {
            return;
        }
        let target = u32::from_le_bytes(rest[1..5].try_into().unwrap());
        let route = if rest[0] == 1 { Route::Audio(target) } else { Route::Plane(target) };
        self.feed_routes.borrow_mut().insert(id, route);
        self.set_active_feeder(id, &rest[5..6]);
    }

    fn feeds_to(&self, target: Route, except: Option<u32>) -> Vec<u32> {
        self.feed_routes
            .borrow()
            .iter()
            .filter(|(feed, route)| **route == target && Some(**feed) != except)
            .map(|(feed, _)| *feed)
            .collect()
    }

    /// Activating one feeder of a target holds the others, the last one asked plays.
    fn hold_feeds_to(&mut self, target: Route, except: Option<u32>) {
        for feed in self.feeds_to(target, except) {
            self.set_feed_active(feed, false);
        }
    }

    /// `[1B active]`. The id names a receiver, a routed feed, or a fed stream by its plane id
    /// (the cluster id for cluster planes).
    fn set_active_feeder(&mut self, id: u32, rest: &[u8]) {
        let active = rest.first().is_some_and(|b| b & 1 != 0);
        let route = self.feed_routes.borrow().get(&id).copied();
        let Some(state) = self.receivers.get(&id).map(|r| r.state.clone()) else {
            if active {
                match route {
                    Some(Route::Plane(plane)) => {
                        self.hold_receivers_of(plane, None);
                        self.set_feed_active(plane, false);
                        self.hold_feeds_to(Route::Plane(plane), Some(id));
                    }
                    Some(Route::Audio(stream)) => {
                        self.hold_feeds_to(Route::Audio(stream), Some(id))
                    }
                    None => {
                        self.hold_receivers_of(id, None);
                        self.hold_feeds_to(Route::Plane(id), None);
                    }
                }
            }
            self.set_feed_active(id, active);
            return;
        };
        let plane_id = state.borrow().plane_id;

        if !active {
            state.borrow_mut().fan.set_active(false);
            return;
        }

        self.hold_receivers_of(plane_id, Some(id));
        self.set_feed_active(plane_id, false);
        self.hold_feeds_to(Route::Plane(plane_id), None);

        let mut st = state.borrow_mut();
        st.fan.set_active(true);
        st.fan.restart();
        if !st.config.is_empty() {
            self.wire.reply(REPLY_CONFIG, plane_id, &st.config);
        }
    }

    fn hold_receivers_of(&self, plane_id: u32, except: Option<u32>) {
        for (other, o) in &self.receivers {
            if Some(*other) != except && o.state.borrow().plane_id == plane_id {
                o.state.borrow_mut().fan.set_active(false);
            }
        }
    }

    /// A feed made active hands its parameter sets on first, the keyframe it waits for needs them.
    fn set_feed_active(&mut self, id: u32, active: bool) {
        self.feed_wanted.borrow_mut().insert(id, active);
        let mut params = Vec::new();
        if let Some(Some(fan)) = self.feed_fans.borrow_mut().get_mut(&id) {
            if active && !fan.is_active() {
                fan.restart();
                params = fan.params().to_vec();
            }
            fan.set_active(active);
        }
        if params.is_empty() {
            return;
        }
        let target = match self.feed_routes.borrow().get(&id) {
            Some(Route::Plane(plane)) => *plane,
            Some(Route::Audio(_)) => return,
            None => id,
        };
        let planes = self.planes.borrow();
        MediaFeed::<O>::for_each_target(&planes, target, |p| {
            for frame in &params {
                p.push(frame);
            }
        });
    }

    /// `[1B codec: 0 aac-lc, 1 opus, 2 lpcm, 3 pcm-le][1B payloadType][4B clockRate]
    /// [1B channels][4B latencyMs][1B flags: bit0=realtime][32B key]
    /// [device name]`. The message id names the stream.
    fn open_audio(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 44;
        const KEY_AT: usize = 12;
        if rest.len() < FIXED {
            return;
        }

        let cfg = AudioConfig {
            codec: match rest[0] {
                1 => AudioCodec::Opus,
                2 => AudioCodec::Lpcm,
                3 => AudioCodec::PcmLe,
                _ => AudioCodec::AacLc,
            },
            payload_type: rest[1],
            clock_rate: u32::from_le_bytes(rest[2..6].try_into().unwrap()),
            channels: rest[6],
            latency_ms: u32::from_le_bytes(rest[7..11].try_into().unwrap()),
            realtime: rest[11] & 1 != 0,
            device: match core::str::from_utf8(&rest[FIXED..]) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
            key: rest[KEY_AT..KEY_AT + 32].try_into().unwrap(),
        };

        let Some(speaker) = self.outside.create_speaker(&cfg) else {
            eprintln!("livi: create audio 0x{id:x} FAILED");
            return;
        };
        if self.visualizer_enabled {
            speaker.set_visualizer_enabled(true);
        }
        let speaker = Output::new(speaker);

        // bit1: the main process feeds this stream.
        let fed = rest[11] & 2 != 0;
        let active = Arc::new(AtomicBool::new(false));
        let feed = AudioFeed::<O> {
            speaker: speaker.clone(),
            wire: self.wire.clone(),
            id,
            active: active.clone(),
        };
        let (ears, data_port, control_port) = if fed {
            (None, 0, 0)
        } else {
            match self.outside.listen_audio(cfg.key, Box::new(feed)) {
                Some((ears, data, control)) => (Some(ears), data, control),
                None => return,
            }
        };

        self.audio
            .borrow_mut()
            .insert(id, AudioStream { speaker, cfg, level: None, active, _ears: ears });
        let mut ports = data_port.to_le_bytes().to_vec();
        ports.extend_from_slice(&control_port.to_le_bytes());
        self.wire.reply(REPLY_AUDIO_PORTS, id, &ports);
    }

    /// The device name, empty for the system's default. A running pipeline cannot change its
    /// sink.
    fn set_audio_output(&mut self, rest: &[u8]) {
        let device = match core::str::from_utf8(rest) {
            Ok("") | Err(_) => None,
            Ok(name) => Some(name.to_owned()),
        };
        for (id, a) in self.audio.borrow_mut().iter_mut() {
            if a.cfg.device == device {
                continue;
            }
            let before = std::mem::replace(&mut a.cfg.device, device.clone());
            let Some(speaker) = self.outside.create_speaker(&a.cfg) else {
                eprintln!("livi: audio 0x{id:x} cannot move to {device:?}, it stays");
                a.cfg.device = before;
                continue;
            };
            if let Some(level) = a.level {
                speaker.set_volume(level, 0);
            }
            if self.visualizer_enabled {
                speaker.set_visualizer_enabled(true);
            }
            a.speaker.replace(speaker);
        }
    }

    /// `[1B codec][1B payloadType][4B sampleRate][1B channels][4B bitrate]
    /// [4B frameMs][2B port][32B key][1B phone length][phone][device]`.
    fn open_uplink(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 50;
        const KEY_AT: usize = 17;
        if rest.len() < FIXED {
            return;
        }
        let phone_len = usize::from(rest[FIXED - 1]);
        if rest.len() < FIXED + phone_len {
            return;
        }
        let (phone, device) = rest[FIXED..].split_at(phone_len);

        let cfg = UplinkConfig {
            codec: if rest[0] == 1 { UplinkCodec::Pcm } else { UplinkCodec::Opus },
            payload_type: rest[1],
            sample_rate: u32::from_le_bytes(rest[2..6].try_into().unwrap()),
            channels: rest[6],
            bitrate: u32::from_le_bytes(rest[7..11].try_into().unwrap()),
            frame_ms: u32::from_le_bytes(rest[11..15].try_into().unwrap()),
            port: u16::from_le_bytes(rest[15..17].try_into().unwrap()),
            key: rest[KEY_AT..KEY_AT + 32].try_into().unwrap(),
            phone: String::from_utf8_lossy(phone).into_owned(),
            device: match core::str::from_utf8(device) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
        };

        match self.outside.open_uplink(cfg) {
            Some(uplink) => {
                self.uplinks.insert(id, uplink);
            }
            None => eprintln!("livi: open microphone 0x{id:x} FAILED"),
        }
    }
    /// [rate u32][channels u8][device length u8][device][path]
    fn open_tap(&mut self, id: u32, rest: &[u8]) {
        const FIXED: usize = 6;
        if rest.len() < FIXED {
            return;
        }
        let device_len = usize::from(rest[FIXED - 1]);
        if rest.len() < FIXED + device_len {
            return;
        }
        let (device, path) = rest[FIXED..].split_at(device_len);
        let Ok(path) = core::str::from_utf8(path) else { return };
        let cfg = TapConfig {
            sample_rate: u32::from_le_bytes(rest[0..4].try_into().unwrap()),
            channels: rest[4],
            device: match core::str::from_utf8(device) {
                Ok("") | Err(_) => None,
                Ok(name) => Some(name.to_owned()),
            },
            path: path.to_owned(),
        };
        match self.outside.open_tap(cfg) {
            Some(tap) => {
                self.taps.insert(id, tap);
            }
            None => eprintln!("livi: open microphone tap 0x{id:x} FAILED"),
        }
    }

    fn set_visualizer_enabled(&mut self, on: bool) {
        self.visualizer_enabled = on;
        self.spectrum.clear();
        for a in self.audio.borrow().values() {
            a.speaker.get().set_visualizer_enabled(on);
        }
    }

    /// The reply carries one f32 per band.
    pub fn pump_visualizer(&mut self) {
        if !self.visualizer_enabled {
            return;
        }
        for (id, a) in self.audio.borrow().iter() {
            if let Some((samples, rate)) = a.speaker.get().take_visualizer() {
                self.spectrum.push(*id, rate, &samples);
            }
        }
        if let Some(bands) = self.spectrum.pump() {
            let payload: Vec<u8> = bands.iter().flat_map(|b| b.to_le_bytes()).collect();
            self.wire.reply(REPLY_VISUALIZER, 0, &payload);
        }
    }

    /// Every URL player tells core where it stands.
    pub fn pump_url_status(&self) {
        for (id, player) in &self.url_players {
            self.wire.reply(REPLY_URL_STATUS, *id, &url_status_body(&player.status()));
        }
    }

    /// Reading starts the counters over.
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
mod tests {
    use super::*;

    const MAIN_PLANE: u32 = 0x7a00_0001;
    const KEYFRAME: u8 = 5;
    const DELTA: u8 = 1;

    #[derive(Default)]
    struct PlaneLog {
        started: usize,
        pushed: Vec<Vec<u8>>,
        gamma: Option<[f64; 5]>,
    }

    #[derive(Default, Clone)]
    struct FakePlane(Rc<RefCell<PlaneLog>>);

    impl FakePlane {
        fn started(&self) -> usize {
            self.0.borrow().started
        }

        fn pushed(&self) -> Vec<Vec<u8>> {
            self.0.borrow().pushed.clone()
        }

        fn gamma(&self) -> Option<[f64; 5]> {
            self.0.borrow().gamma
        }
    }

    impl Plane for FakePlane {
        fn start(&self) {
            self.0.borrow_mut().started += 1;
        }

        fn push(&self, nal: &[u8]) {
            self.0.borrow_mut().pushed.push(nal.to_vec());
        }

        fn flush(&self) {
            self.0.borrow_mut().pushed.clear();
        }

        fn set_gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
            self.0.borrow_mut().gamma = Some([gamma, contrast, r, g, b]);
        }
    }

    #[derive(Default)]
    struct UrlLog {
        started: usize,
        status: UrlStatus,
        rates: Vec<f64>,
        seeks: Vec<f64>,
        muted: Option<bool>,
        dropped: bool,
    }

    /// The world keeps the log, the host the player.
    struct FakeUrlPlayer(Rc<RefCell<UrlLog>>);

    impl UrlPlay for FakeUrlPlayer {
        fn status(&self) -> UrlStatus {
            self.0.borrow().status
        }

        fn start(&self) {
            self.0.borrow_mut().started += 1;
        }

        fn set_rate(&self, rate: f64) {
            self.0.borrow_mut().rates.push(rate);
        }

        fn seek(&self, seconds: f64) {
            self.0.borrow_mut().seeks.push(seconds);
        }

        fn set_muted(&self, muted: bool) {
            self.0.borrow_mut().muted = Some(muted);
        }
    }

    impl Drop for FakeUrlPlayer {
        fn drop(&mut self) {
            self.0.borrow_mut().dropped = true;
        }
    }

    type SharedSink = Rc<RefCell<Box<dyn ScreenSink>>>;
    type SharedAudioSink = Rc<RefCell<Box<dyn AudioSink + Send>>>;

    #[derive(Default)]
    struct SpeakerLog {
        pushed: Vec<Vec<u8>>,
        volume: Option<f64>,
        ramp_ms: Option<u64>,
        visualizer_on: Option<bool>,
        visualizer: Vec<u8>,
    }

    #[derive(Default, Clone)]
    struct FakeSpeaker(Arc<std::sync::Mutex<SpeakerLog>>);

    impl FakeSpeaker {
        fn pushed(&self) -> Vec<Vec<u8>> {
            self.0.lock().unwrap().pushed.clone()
        }

        fn volume(&self) -> Option<f64> {
            self.0.lock().unwrap().volume
        }

        fn ramp_ms(&self) -> Option<u64> {
            self.0.lock().unwrap().ramp_ms
        }

        fn visualizer_on(&self) -> Option<bool> {
            self.0.lock().unwrap().visualizer_on
        }

        fn feed_visualizer(&self, samples: &[u8]) {
            self.0.lock().unwrap().visualizer.extend_from_slice(samples);
        }
    }

    struct FakeUplink(Rc<RefCell<usize>>);

    impl Drop for FakeUplink {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }

    struct FakeTap(Rc<RefCell<usize>>);

    impl Drop for FakeTap {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }

    impl Speaker for FakeSpeaker {
        fn push_rtp(&self, rtp: &[u8]) {
            self.0.lock().unwrap().pushed.push(rtp.to_vec());
        }

        fn push_samples(&self, samples: &[u8]) {
            self.0.lock().unwrap().pushed.push(samples.to_vec());
        }

        fn set_volume(&self, level: f64, ms: u64) {
            let mut w = self.0.lock().unwrap();
            w.volume = Some(level);
            w.ramp_ms = Some(ms);
        }

        fn set_visualizer_enabled(&self, on: bool) {
            self.0.lock().unwrap().visualizer_on = Some(on);
        }

        fn take_visualizer(&self) -> Option<(Vec<u8>, u32)> {
            let s = core::mem::take(&mut self.0.lock().unwrap().visualizer);
            if s.is_empty() { None } else { Some((s, 48000)) }
        }
    }

    #[derive(Default)]
    struct World {
        planes: Vec<FakePlane>,
        codecs: Vec<(u32, String, Vec<u8>)>,
        sinks: Vec<SharedSink>,
        keys: Vec<[u8; 32]>,
        speakers: Vec<FakeSpeaker>,
        audio_cfgs: Vec<OpenedAudio>,
        audio_sinks: Vec<SharedAudioSink>,
        audio_keys: Vec<[u8; 32]>,
        refuse_plane: bool,
        uplinks: Vec<OpenedUplink>,
        uplinks_dropped: Rc<RefCell<usize>>,
        taps: Vec<(u32, u8, Option<String>, String)>,
        taps_dropped: Rc<RefCell<usize>>,
        refuse_tap: bool,
        refuse_speaker: bool,
        refuse_uplink: bool,
        deaf: bool,
        audio_deaf: bool,
        port: u16,
        audio_ports: (u16, u16),
        feeds: Vec<SharedMediaSink>,
        feed_paths: Vec<String>,
        refuse_feed: bool,
        /// Plane, url and audio device.
        urls: Vec<(u32, String, String)>,
        url_logs: Vec<Rc<RefCell<UrlLog>>>,
        refuse_url: bool,
    }

    type SharedMediaSink = Rc<RefCell<Box<dyn MediaSink>>>;

    #[derive(Default, Clone)]
    struct Fake(Rc<RefCell<World>>);

    impl Outside for Fake {
        type Plane = FakePlane;
        type UrlPlayer = FakeUrlPlayer;
        type Ears = SharedSink;
        type Speaker = FakeSpeaker;
        type AudioEars = SharedAudioSink;
        type Uplink = FakeUplink;
        type Tap = FakeTap;

        fn open_tap(&self, cfg: TapConfig) -> Option<FakeTap> {
            if self.0.borrow().refuse_tap {
                return None;
            }
            let mut w = self.0.borrow_mut();
            w.taps.push((cfg.sample_rate, cfg.channels, cfg.device, cfg.path));
            Some(FakeTap(w.taps_dropped.clone()))
        }

        fn create_plane(&self, id: u32, codec: &str, codec_data: &[u8]) -> Option<FakePlane> {
            if self.0.borrow().refuse_plane {
                return None;
            }
            let plane = FakePlane::default();
            let mut w = self.0.borrow_mut();
            w.codecs.push((id, codec.to_owned(), codec_data.to_vec()));
            w.planes.push(plane.clone());
            Some(plane)
        }

        fn play_url(&self, id: u32, url: &str, audio_device: &str) -> Option<FakeUrlPlayer> {
            if self.0.borrow().refuse_url {
                return None;
            }
            let log = Rc::new(RefCell::new(UrlLog::default()));
            let mut w = self.0.borrow_mut();
            w.urls.push((id, url.to_owned(), audio_device.to_owned()));
            w.url_logs.push(log.clone());
            Some(FakeUrlPlayer(log))
        }

        fn listen(&self, key: [u8; 32], sink: Box<dyn ScreenSink>) -> Option<(SharedSink, u16)> {
            if self.0.borrow().deaf {
                return None;
            }
            let shared: SharedSink = Rc::new(RefCell::new(sink));
            let mut w = self.0.borrow_mut();
            w.keys.push(key);
            w.sinks.push(shared.clone());
            Some((shared, w.port))
        }

        fn open_uplink(&self, cfg: UplinkConfig) -> Option<FakeUplink> {
            if self.0.borrow().refuse_uplink {
                return None;
            }
            let mut w = self.0.borrow_mut();
            w.uplinks.push((
                cfg.payload_type,
                cfg.sample_rate,
                cfg.channels,
                cfg.bitrate,
                cfg.frame_ms,
                cfg.port,
                cfg.phone,
                cfg.device,
            ));
            Some(FakeUplink(w.uplinks_dropped.clone()))
        }

        fn create_speaker(&self, cfg: &AudioConfig) -> Option<FakeSpeaker> {
            if self.0.borrow().refuse_speaker {
                return None;
            }
            let speaker = FakeSpeaker::default();
            let mut w = self.0.borrow_mut();
            w.audio_cfgs.push((
                cfg.codec,
                cfg.payload_type,
                cfg.clock_rate,
                cfg.channels,
                cfg.latency_ms,
                cfg.realtime,
                cfg.device.clone(),
            ));
            w.speakers.push(speaker.clone());
            Some(speaker)
        }

        fn listen_audio(
            &self,
            key: [u8; 32],
            sink: Box<dyn AudioSink + Send>,
        ) -> Option<(SharedAudioSink, u16, u16)> {
            if self.0.borrow().audio_deaf {
                return None;
            }
            let shared: SharedAudioSink = Rc::new(RefCell::new(sink));
            let mut w = self.0.borrow_mut();
            w.audio_keys.push(key);
            w.audio_sinks.push(shared.clone());
            Some((shared, w.audio_ports.0, w.audio_ports.1))
        }

        type FeedEars = SharedMediaSink;

        fn open_feed(&self, path: &str, sink: Box<dyn MediaSink>) -> Option<SharedMediaSink> {
            if self.0.borrow().refuse_feed {
                return None;
            }
            let shared: SharedMediaSink = Rc::new(RefCell::new(sink));
            let mut w = self.0.borrow_mut();
            w.feed_paths.push(path.to_owned());
            w.feeds.push(shared.clone());
            Some(shared)
        }
    }

    type Reply = (u8, u32, Vec<u8>);
    /// Payload type, rate, channels, bitrate, frame length, port, phone and device.
    type OpenedUplink = (u8, u32, u8, u32, u32, u16, String, Option<String>);
    /// Codec, payload type, rate, channels, buffer depth, realtime and device.
    type OpenedAudio = (AudioCodec, u8, u32, u8, u32, bool, Option<String>);

    #[derive(Default, Clone)]
    struct Sent(Arc<std::sync::Mutex<Vec<Reply>>>);

    impl Wire for Sent {
        fn reply(&self, op: u8, id: u32, rest: &[u8]) {
            self.0.lock().unwrap().push((op, id, rest.to_vec()));
        }
    }

    fn frame(op: u8, id: u32, rest: &[u8]) -> Vec<u8> {
        let mut v = ((HEAD + rest.len()) as u32).to_ne_bytes().to_vec();
        v.push(op);
        v.extend_from_slice(&id.to_ne_bytes());
        v.extend_from_slice(rest);
        v
    }

    const HEAD: usize = 5;

    fn create_body(codec: &str, codec_data: &[u8]) -> Vec<u8> {
        let mut v = vec![codec.len() as u8];
        v.extend_from_slice(codec.as_bytes());
        v.extend_from_slice(codec_data);
        v
    }

    fn listen_body(plane_id: u32, cluster: bool, key: u8) -> Vec<u8> {
        let mut v = plane_id.to_ne_bytes().to_vec();
        v.push(u8::from(cluster));
        v.extend_from_slice(&[key; 32]);
        v
    }

    fn gamma_body(v: [f64; 5]) -> Vec<u8> {
        v.iter().flat_map(|d| d.to_ne_bytes()).collect()
    }

    /// One access unit: a four-byte length, the NAL header byte, and a marker
    /// that tells the frames apart.
    fn nal(kind: u8, mark: u8) -> Vec<u8> {
        let mut v = 2u32.to_be_bytes().to_vec();
        v.push(kind);
        v.push(mark);
        v
    }

    mod feed_tests {
        use super::*;

        const CLUSTER_A: u32 = 0x7a00_0011;
        const CLUSTER_B: u32 = 0x7a00_0012;

        impl Fixture {
            fn open_feed(&mut self, path: &str) {
                self.send(OP_FEED_OPEN, 9, path.as_bytes());
            }

            fn feed_in(&self, kind: u8, id: u32, payload: &[u8]) {
                let sink = self.world.0.borrow().feeds.last().unwrap().clone();
                sink.borrow_mut().on_record(feedproto::Record {
                    kind,
                    id,
                    ts: 1,
                    payload: payload.to_vec(),
                });
            }
        }

        fn audio_body(fed: bool) -> Vec<u8> {
            let mut v = vec![3u8, 0];
            v.extend_from_slice(&48_000u32.to_le_bytes());
            v.push(2);
            v.extend_from_slice(&0u32.to_le_bytes());
            v.push(if fed { 2 } else { 0 });
            v.extend_from_slice(&[0u8; 32]);
            v
        }

        #[test]
        fn the_feed_statistics_name_the_stream_and_start_over_when_read() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(DELTA, 1));

            let first = f.host.take_stats();

            assert_eq!(
                first,
                vec![
                    "[feed] recv 0x7a000001: in=1 dropped=1 pushed=0 awaiting_kf=1 active=1"
                        .to_owned()
                ]
            );
            assert!(f.host.take_stats().is_empty());
        }

        #[test]
        fn opening_the_feed_answers_with_the_path() {
            let mut f = Fixture::new();
            f.open_feed("/tmp/x.feed");
            assert_eq!(f.replies(), vec![(REPLY_FEED, 9, b"/tmp/x.feed".to_vec())]);
            assert_eq!(f.world.0.borrow().feed_paths, vec!["/tmp/x.feed".to_owned()]);
        }

        #[test]
        fn a_feed_the_world_refuses_answers_empty() {
            let mut f = Fixture::new();
            f.world.0.borrow_mut().refuse_feed = true;
            f.open_feed("/tmp/x.feed");
            assert_eq!(f.replies(), vec![(REPLY_FEED, 9, vec![])]);
        }

        #[test]
        fn fed_video_waits_for_a_keyframe_then_reaches_the_plane() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(DELTA, 1));
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 2));
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(DELTA, 3));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 2), nal(DELTA, 3)]);
        }

        #[test]
        fn a_codec_the_gate_cannot_read_passes_everything() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("vp9", &[]));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0xff]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(DELTA, 1));
            assert_eq!(f.plane(0).pushed(), vec![nal(DELTA, 1)]);
        }

        #[test]
        fn the_cluster_id_fans_out_to_every_cluster_plane() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, CLUSTER_A, &create_body("h264", &[]));
            f.send(OP_CREATE, CLUSTER_B, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, CLUSTER_RECV_ID, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, CLUSTER_RECV_ID, &nal(KEYFRAME, 1));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
            assert_eq!(f.plane(1).pushed(), vec![nal(KEYFRAME, 1)]);
        }

        #[test]
        fn a_plane_created_late_is_primed_from_the_feed() {
            let mut f = Fixture::new();
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 1));
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
        }

        #[test]
        fn a_receiver_made_active_holds_the_fed_stream_of_its_plane() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 1));

            f.feeder(42, MAIN_PLANE, false);
            f.config(0, CpCodec::H264, &[1, 2]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 2));
            f.frame_in(0, &nal(KEYFRAME, 3));

            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1), nal(KEYFRAME, 3)]);
        }

        #[test]
        fn a_fed_stream_made_active_holds_the_receivers_and_waits_for_a_keyframe() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.feeder(42, MAIN_PLANE, false);
            f.config(0, CpCodec::H264, &[1, 2]);
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 1));
            assert!(f.plane(0).pushed().is_empty());

            f.send(OP_SET_ACTIVE, MAIN_PLANE, &[1]);
            f.frame_in(0, &nal(KEYFRAME, 2));
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(DELTA, 3));
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 4));

            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 4)]);
        }

        #[test]
        fn a_fed_stream_that_starts_later_takes_the_answer_given_before() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.send(OP_SET_ACTIVE, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO_START, MAIN_PLANE, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 1));
            assert!(f.plane(0).pushed().is_empty());

            f.send(OP_SET_ACTIVE, MAIN_PLANE, &[1]);
            f.feed_in(feedproto::KIND_VIDEO, MAIN_PLANE, &nal(KEYFRAME, 2));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 2)]);
        }

        #[test]
        fn fed_audio_reaches_only_an_active_stream() {
            let mut f = Fixture::new();
            f.send(OP_AUDIO_OPEN, 77, &audio_body(true));
            f.open_feed("/tmp/x.feed");
            f.feed_in(feedproto::KIND_AUDIO, 77, &[1, 2]);
            assert!(f.speaker(0).pushed().is_empty());
            f.send(OP_AUDIO_ACTIVE, 77, &[1]);
            f.feed_in(feedproto::KIND_AUDIO, 77, &[3, 4]);
            assert_eq!(f.speaker(0).pushed(), vec![vec![3, 4]]);
        }

        fn route(kind: u8, target: u32, active: bool) -> Vec<u8> {
            let mut v = vec![kind];
            v.extend_from_slice(&target.to_le_bytes());
            v.push(u8::from(active));
            v
        }

        const OLD: u32 = 0x7b00_0001;
        const NEW: u32 = 0x7b00_0002;

        #[test]
        fn a_late_hold_of_the_old_feed_leaves_the_new_one_playing() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, OLD, &route(0, MAIN_PLANE, true));
            f.feed_in(feedproto::KIND_VIDEO_START, OLD, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, OLD, &nal(KEYFRAME, 1));

            f.send(OP_FEED_ROUTE, NEW, &route(0, MAIN_PLANE, true));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, OLD, &nal(KEYFRAME, 2));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 3));
            f.send(OP_SET_ACTIVE, OLD, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(DELTA, 4));

            assert_eq!(
                f.plane(0).pushed(),
                vec![nal(KEYFRAME, 1), nal(KEYFRAME, 3), nal(DELTA, 4)]
            );
            let stats = f.host.take_stats();
            assert!(stats.iter().any(|l| l.starts_with("[feed] recv 0x7b000002 -> 0x7a000001:")));
        }

        #[test]
        fn a_routed_feed_waits_until_it_is_made_active() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, NEW, &route(0, MAIN_PLANE, false));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 1));
            assert!(f.plane(0).pushed().is_empty());

            f.send(OP_SET_ACTIVE, NEW, &[1]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 2));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 2)]);
        }

        #[test]
        fn a_receiver_and_a_routed_feed_hold_each_other() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, NEW, &route(0, MAIN_PLANE, true));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);

            f.feeder(42, MAIN_PLANE, false);
            f.config(0, CpCodec::H264, &[1, 2]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 1));
            f.frame_in(0, &nal(KEYFRAME, 2));
            f.send(OP_SET_ACTIVE, NEW, &[1]);
            f.frame_in(0, &nal(KEYFRAME, 3));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 4));

            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 2), nal(KEYFRAME, 4)]);
        }

        #[test]
        fn a_plane_created_late_is_primed_from_the_active_routed_feed() {
            let mut f = Fixture::new();
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, NEW, &route(0, CLUSTER_RECV_ID, true));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 1));
            f.send(OP_CREATE, CLUSTER_A, &create_body("h264", &[]));
            assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
        }

        const SPS: u8 = 7;

        #[test]
        fn a_routed_feed_made_active_hands_its_parameter_sets_on_first() {
            let mut f = Fixture::new();
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, NEW, &route(0, MAIN_PLANE, false));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(SPS, 1));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 2));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(DELTA, 3));
            assert!(f.plane(0).pushed().is_empty());

            f.send(OP_SET_ACTIVE, NEW, &[1]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(DELTA, 4));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 5));

            assert_eq!(f.plane(0).pushed(), vec![nal(SPS, 1), nal(KEYFRAME, 5)]);
        }

        #[test]
        fn a_new_plane_gets_the_parameter_sets_of_the_feed_it_plays() {
            let mut f = Fixture::new();
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, NEW, &route(0, MAIN_PLANE, true));
            f.feed_in(feedproto::KIND_VIDEO_START, NEW, &[0]);
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(SPS, 1));
            f.feed_in(feedproto::KIND_VIDEO, NEW, &nal(KEYFRAME, 2));
            f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
            assert_eq!(f.plane(0).pushed(), vec![nal(SPS, 1), nal(KEYFRAME, 2)]);
        }

        #[test]
        fn routed_audio_reaches_the_stream_only_from_the_active_feed() {
            let mut f = Fixture::new();
            f.send(OP_AUDIO_OPEN, 77, &audio_body(true));
            f.send(OP_AUDIO_ACTIVE, 77, &[1]);
            f.open_feed("/tmp/x.feed");
            f.send(OP_FEED_ROUTE, OLD, &route(1, 77, true));
            f.send(OP_FEED_ROUTE, NEW, &route(1, 77, true));
            f.feed_in(feedproto::KIND_AUDIO, OLD, &[1]);
            f.feed_in(feedproto::KIND_AUDIO, NEW, &[2]);
            f.send(OP_SET_ACTIVE, OLD, &[0]);
            f.feed_in(feedproto::KIND_AUDIO, NEW, &[3]);

            f.send(OP_FEED_CLOSE, NEW, &[]);
            f.feed_in(feedproto::KIND_AUDIO, NEW, &[4]);
            assert_eq!(f.speaker(0).pushed(), vec![vec![2], vec![3]]);
        }

        #[test]
        fn a_short_route_is_ignored() {
            let mut f = Fixture::new();
            f.send(OP_FEED_ROUTE, NEW, &[0, 1, 0]);
            assert!(f.host.feed_routes.borrow().is_empty());
        }
    }

    struct Fixture {
        host: Host<Fake>,
        world: Fake,
        sent: Sent,
    }

    impl Fixture {
        fn new() -> Self {
            let world = Fake::default();
            world.0.borrow_mut().port = 5555;
            world.0.borrow_mut().audio_ports = (6000, 6001);
            let sent = Sent::default();
            let host = Host::new(world.clone(), Arc::new(sent.clone()));
            Self { host, world, sent }
        }

        fn send(&mut self, op: u8, id: u32, rest: &[u8]) {
            self.host.feed(&frame(op, id, rest));
        }

        fn plane(&self, i: usize) -> FakePlane {
            self.world.0.borrow().planes[i].clone()
        }

        fn planes(&self) -> usize {
            self.world.0.borrow().planes.len()
        }

        fn sink(&self, i: usize) -> SharedSink {
            self.world.0.borrow().sinks[i].clone()
        }

        fn config(&self, i: usize, codec: CpCodec, atom: &[u8]) {
            self.sink(i).borrow_mut().on_config(codec, atom);
        }

        fn frame_in(&self, i: usize, nal: &[u8]) {
            self.sink(i).borrow_mut().on_frame(nal);
        }

        fn started_in(&self, i: usize) {
            self.sink(i).borrow_mut().on_started();
        }

        fn replies(&self) -> Vec<Reply> {
            self.sent.0.lock().unwrap().clone()
        }

        fn feeder(&mut self, recv_id: u32, plane_id: u32, cluster: bool) {
            self.send(OP_LISTEN, recv_id, &listen_body(plane_id, cluster, 1));
            self.send(OP_SET_ACTIVE, recv_id, &[1]);
        }
    }

    #[test]
    fn a_create_message_starts_a_plane_and_data_reaches_it() {
        let mut f = Fixture::new();

        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[9, 9]));
        f.send(OP_DATA, MAIN_PLANE, &[1, 2, 3]);

        assert_eq!(f.world.0.borrow().codecs, vec![(MAIN_PLANE, "h264".to_owned(), vec![9, 9])]);
        assert_eq!(f.plane(0).started(), 1);
        assert_eq!(f.plane(0).pushed(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn creating_a_plane_twice_replaces_the_first() {
        let mut f = Fixture::new();

        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h265", &[]));
        f.send(OP_DATA, MAIN_PLANE, &[7]);

        assert_eq!(f.planes(), 2);
        assert!(f.plane(0).pushed().is_empty());
        assert_eq!(f.plane(1).pushed(), vec![vec![7]]);
    }

    #[test]
    fn a_stop_message_drops_the_plane() {
        let mut f = Fixture::new();
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.send(OP_STOP, MAIN_PLANE, &[]);
        f.send(OP_DATA, MAIN_PLANE, &[7]);

        assert!(f.plane(0).pushed().is_empty());
    }

    const VIDEO_PLANE: u32 = 0x7a00_0002;

    fn url_log(f: &Fixture, i: usize) -> Rc<RefCell<UrlLog>> {
        f.world.0.borrow().url_logs[i].clone()
    }

    #[test]
    fn a_play_message_starts_a_player_on_its_url_and_audio_device() {
        let mut f = Fixture::new();

        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/i.m3u8", "car"));

        assert_eq!(
            f.world.0.borrow().urls,
            vec![(VIDEO_PLANE, "http://h/i.m3u8".to_owned(), "car".to_owned())]
        );
        assert_eq!(url_log(&f, 0).borrow().started, 1);
    }

    #[test]
    fn rate_seek_and_mute_reach_the_player() {
        let mut f = Fixture::new();
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));

        f.send(OP_PLAY_RATE, VIDEO_PLANE, &0.0f64.to_le_bytes());
        f.send(OP_PLAY_SEEK, VIDEO_PLANE, &12.5f64.to_le_bytes());
        f.send(OP_PLAY_MUTE, VIDEO_PLANE, &[1]);

        let log = url_log(&f, 0);
        let log = log.borrow();
        assert_eq!((log.rates.as_slice(), log.seeks.as_slice()), (&[0.0][..], &[12.5][..]));
        assert_eq!(log.muted, Some(true));
    }

    #[test]
    fn control_messages_short_of_their_value_or_their_player_are_ignored() {
        let mut f = Fixture::new();
        f.send(OP_PLAY_RATE, VIDEO_PLANE, &1.0f64.to_le_bytes());
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));

        f.send(OP_PLAY_RATE, VIDEO_PLANE, &[0; 4]);
        f.send(OP_PLAY_SEEK, VIDEO_PLANE, &[]);
        f.send(OP_PLAY_MUTE, VIDEO_PLANE, &[]);
        f.send(OP_PLAY_MUTE, MAIN_PLANE, &[1]);

        let log = url_log(&f, 0);
        let log = log.borrow();
        assert!(log.rates.is_empty() && log.seeks.is_empty());
        assert_eq!(log.muted, None);
    }

    #[test]
    fn every_url_player_reports_its_status() {
        let mut f = Fixture::new();
        f.host.pump_url_status();
        assert!(f.replies().is_empty());
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));
        let status =
            UrlStatus { position: Some(4.0), ready: true, playing: true, ..Default::default() };
        url_log(&f, 0).borrow_mut().status = status;

        f.host.pump_url_status();

        assert_eq!(f.replies(), vec![(REPLY_URL_STATUS, VIDEO_PLANE, url_status_body(&status))]);
    }

    #[test]
    fn a_stop_message_drops_the_player() {
        let mut f = Fixture::new();
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));

        f.send(OP_STOP, VIDEO_PLANE, &[]);
        f.send(OP_PLAY_MUTE, VIDEO_PLANE, &[1]);

        assert!(url_log(&f, 0).borrow().dropped);
        assert_eq!(url_log(&f, 0).borrow().muted, None);
    }

    #[test]
    fn a_player_and_a_plane_on_one_id_replace_each_other() {
        let mut f = Fixture::new();
        f.send(OP_CREATE, VIDEO_PLANE, &create_body("h264", &[]));

        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));
        f.send(OP_DATA, VIDEO_PLANE, &[7]);
        assert!(f.plane(0).pushed().is_empty());

        f.send(OP_CREATE, VIDEO_PLANE, &create_body("h264", &[]));
        assert!(url_log(&f, 0).borrow().dropped);
    }

    #[test]
    fn a_play_message_without_a_url_or_a_player_leaves_nothing_behind() {
        let mut f = Fixture::new();
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("", "car"));
        f.world.0.borrow_mut().refuse_url = true;
        f.send(OP_PLAY_URL, VIDEO_PLANE, &livi_host_proto::url_body("http://h/", ""));

        assert!(f.world.0.borrow().url_logs.is_empty());
        assert!(f.host.url_players.is_empty());
    }

    #[test]
    fn a_create_message_shorter_than_its_codec_name_is_ignored() {
        let mut f = Fixture::new();

        f.send(OP_CREATE, MAIN_PLANE, &[8, b'h']);

        assert_eq!(f.planes(), 0);
    }

    #[test]
    fn a_plane_that_cannot_be_built_leaves_nothing_behind() {
        let mut f = Fixture::new();
        f.world.0.borrow_mut().refuse_plane = true;

        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.send(OP_DATA, MAIN_PLANE, &[7]);

        assert_eq!(f.planes(), 0);
    }

    #[test]
    fn gamma_reaches_the_plane() {
        let mut f = Fixture::new();
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.send(OP_GAMMA, MAIN_PLANE, &gamma_body([1.5, 2.0, 0.5, 0.6, 0.7]));

        assert_eq!(f.plane(0).gamma(), Some([1.5, 2.0, 0.5, 0.6, 0.7]));
    }

    #[test]
    fn a_gamma_message_short_of_five_values_is_ignored() {
        let mut f = Fixture::new();
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.send(OP_GAMMA, MAIN_PLANE, &gamma_body([1.5, 2.0, 0.5, 0.6, 0.7])[..39]);

        assert_eq!(f.plane(0).gamma(), None);
    }

    #[test]
    fn an_unknown_opcode_is_ignored() {
        let mut f = Fixture::new();

        f.send(9, MAIN_PLANE, &[1, 2, 3]);

        assert_eq!(f.planes(), 0);
        assert!(f.replies().is_empty());
    }

    #[test]
    fn two_messages_in_one_chunk_are_both_acted_on() {
        let mut f = Fixture::new();
        let mut chunk = frame(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        chunk.extend(frame(OP_DATA, MAIN_PLANE, &[4]));

        f.host.feed(&chunk);

        assert_eq!(f.plane(0).pushed(), vec![vec![4]]);
    }

    #[test]
    fn a_message_split_across_chunks_waits_for_the_rest() {
        let mut f = Fixture::new();
        let msg = frame(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.host.feed(&msg[..6]);
        assert_eq!(f.planes(), 0);

        f.host.feed(&msg[6..]);
        assert_eq!(f.planes(), 1);
    }

    #[test]
    fn listening_answers_with_the_port_and_takes_the_key() {
        let mut f = Fixture::new();

        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 3));

        assert_eq!(f.replies(), vec![(REPLY_PORT, 42, 5555u16.to_le_bytes().to_vec())]);
        assert_eq!(f.world.0.borrow().keys, vec![[3u8; 32]]);
    }

    #[test]
    fn a_listen_message_short_of_the_key_opens_nothing() {
        let mut f = Fixture::new();

        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 3)[..36]);

        assert!(f.replies().is_empty());
        assert!(f.world.0.borrow().sinks.is_empty());
    }

    #[test]
    fn a_receiver_that_cannot_listen_is_not_announced() {
        let mut f = Fixture::new();
        f.world.0.borrow_mut().deaf = true;

        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 3));

        assert!(f.replies().is_empty());
    }

    #[test]
    fn frames_reach_the_plane_the_receiver_serves() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));
        f.frame_in(0, &nal(DELTA, 2));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1), nal(DELTA, 2)]);
    }

    #[test]
    fn frames_before_a_keyframe_are_dropped() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(DELTA, 1));

        assert!(f.plane(0).pushed().is_empty());
    }

    #[test]
    fn a_passive_receiver_feeds_nothing() {
        let mut f = Fixture::new();
        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 1));
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));

        assert!(f.plane(0).pushed().is_empty());
    }

    #[test]
    fn a_cluster_receiver_feeds_every_cluster_plane() {
        let mut f = Fixture::new();
        f.feeder(CLUSTER_RECV_ID, CLUSTER_RECV_ID, true);
        f.send(OP_CREATE, CLUSTER_PLANE_MIN, &create_body("h264", &[]));
        f.send(OP_CREATE, CLUSTER_PLANE_MAX, &create_body("h264", &[]));

        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
        assert_eq!(f.plane(1).pushed(), vec![nal(KEYFRAME, 1)]);
    }

    #[test]
    fn a_plane_created_mid_stream_is_primed_with_the_cached_gop() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));
        f.frame_in(0, &nal(DELTA, 2));

        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1), nal(DELTA, 2)]);
    }

    #[test]
    fn a_cluster_plane_is_primed_from_the_cluster_receiver() {
        let mut f = Fixture::new();
        f.feeder(CLUSTER_RECV_ID, CLUSTER_RECV_ID, true);
        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));

        f.send(OP_CREATE, CLUSTER_PLANE_MIN, &create_body("h264", &[]));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
    }

    #[test]
    fn a_torn_down_receiver_no_longer_primes_new_planes() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));

        f.send(OP_TEARDOWN, 42, &[]);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));

        assert!(f.plane(0).pushed().is_empty());
    }

    #[test]
    fn activating_a_receiver_makes_the_other_one_of_the_plane_passive() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.config(0, CpCodec::H264, &[1, 2]);

        f.feeder(43, MAIN_PLANE, false);
        f.config(1, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));
        f.frame_in(1, &nal(KEYFRAME, 2));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 2)]);
    }

    #[test]
    fn a_receiver_made_active_waits_for_the_next_keyframe() {
        let mut f = Fixture::new();
        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 1));
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(KEYFRAME, 1));
        f.frame_in(0, &nal(DELTA, 2));
        assert!(f.plane(0).pushed().is_empty());

        f.send(OP_SET_ACTIVE, 42, &[1]);

        assert!(f.plane(0).pushed().is_empty());
        f.frame_in(0, &nal(DELTA, 3));
        assert!(f.plane(0).pushed().is_empty());
        f.frame_in(0, &nal(KEYFRAME, 4));
        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 4)]);
    }

    #[test]
    fn a_receiver_of_another_plane_stays_active() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.config(0, CpCodec::H264, &[1, 2]);

        f.feeder(43, CLUSTER_RECV_ID, true);
        f.frame_in(0, &nal(KEYFRAME, 1));

        assert_eq!(f.plane(0).pushed(), vec![nal(KEYFRAME, 1)]);
    }

    #[test]
    fn deactivating_a_receiver_stops_its_feed() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.send(OP_CREATE, MAIN_PLANE, &create_body("h264", &[]));
        f.config(0, CpCodec::H264, &[1, 2]);

        f.send(OP_SET_ACTIVE, 42, &[0]);
        f.frame_in(0, &nal(KEYFRAME, 1));

        assert!(f.plane(0).pushed().is_empty());
    }

    #[test]
    fn setting_a_receiver_that_does_not_exist_does_nothing() {
        let mut f = Fixture::new();

        f.send(OP_SET_ACTIVE, 42, &[1]);

        assert!(f.replies().is_empty());
    }

    #[test]
    fn a_configuration_is_forwarded_while_the_receiver_is_active() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);

        f.config(0, CpCodec::H265, &[1, 2]);

        assert_eq!(
            f.replies().last(),
            Some(&(REPLY_CONFIG, MAIN_PLANE, vec![CpCodec::H265 as u8, 1, 2]))
        );
    }

    #[test]
    fn activating_a_receiver_forwards_the_configuration_it_already_had() {
        let mut f = Fixture::new();
        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 1));
        f.config(0, CpCodec::H264, &[7, 8]);

        f.send(OP_SET_ACTIVE, 42, &[1]);

        assert_eq!(
            f.replies().last(),
            Some(&(REPLY_CONFIG, MAIN_PLANE, vec![CpCodec::H264 as u8, 7, 8]))
        );
    }

    #[test]
    fn activating_a_receiver_without_a_configuration_forwards_nothing() {
        let mut f = Fixture::new();
        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 1));

        f.send(OP_SET_ACTIVE, 42, &[1]);

        assert_eq!(f.replies().len(), 1);
    }

    #[test]
    fn a_keepalive_configuration_keeps_the_last_record() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.config(0, CpCodec::H264, &[7, 8]);

        f.config(0, CpCodec::H264, &[]);
        f.send(OP_SET_ACTIVE, 42, &[1]);

        assert_eq!(
            f.replies().last(),
            Some(&(REPLY_CONFIG, MAIN_PLANE, vec![CpCodec::H264 as u8, 7, 8]))
        );
    }

    #[test]
    fn the_start_of_a_stream_is_reported_while_the_receiver_is_active() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);

        f.started_in(0);

        assert_eq!(f.replies().last(), Some(&(REPLY_STARTED, MAIN_PLANE, vec![])));
    }

    #[test]
    fn a_passive_receiver_reports_no_start() {
        let mut f = Fixture::new();
        f.send(OP_LISTEN, 42, &listen_body(MAIN_PLANE, false, 1));

        f.started_in(0);

        assert_eq!(f.replies().len(), 1);
    }

    fn audio_body(codec: u8, realtime: bool, device: &str) -> Vec<u8> {
        let mut v = vec![codec, 96];
        v.extend_from_slice(&44100u32.to_le_bytes());
        v.push(2);
        v.extend_from_slice(&1000u32.to_le_bytes());
        v.push(u8::from(realtime));
        v.extend_from_slice(&[5u8; 32]);
        v.extend_from_slice(device.as_bytes());
        v
    }

    const MUSIC: u32 = 0x7b00_0001;

    impl Fixture {
        fn speaker(&self, i: usize) -> FakeSpeaker {
            self.world.0.borrow().speakers[i].clone()
        }

        fn audio_sink(&self, i: usize) -> SharedAudioSink {
            self.world.0.borrow().audio_sinks[i].clone()
        }
    }

    #[test]
    fn opening_an_audio_stream_answers_with_both_ports_and_takes_the_key() {
        let mut f = Fixture::new();

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        let mut ports = 6000u16.to_le_bytes().to_vec();
        ports.extend_from_slice(&6001u16.to_le_bytes());
        assert_eq!(f.replies(), vec![(REPLY_AUDIO_PORTS, MUSIC, ports)]);
        assert_eq!(f.world.0.borrow().audio_keys, vec![[5u8; 32]]);
    }

    #[test]
    fn the_stream_settings_reach_the_pipeline() {
        let mut f = Fixture::new();

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(1, true, "alsa_output.front"));

        assert_eq!(
            f.world.0.borrow().audio_cfgs,
            vec![(
                AudioCodec::Opus,
                96,
                44100,
                2,
                1000,
                true,
                Some("alsa_output.front".to_owned())
            )]
        );
    }

    #[test]
    fn the_codec_byte_picks_the_pipeline() {
        for (byte, expected) in
            [(0u8, AudioCodec::AacLc), (1, AudioCodec::Opus), (2, AudioCodec::Lpcm)]
        {
            let mut f = Fixture::new();
            f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(byte, false, ""));

            assert_eq!(f.world.0.borrow().audio_cfgs[0].0, expected);
        }
    }

    #[test]
    fn an_audio_message_short_of_the_key_opens_nothing() {
        let mut f = Fixture::new();

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, "")[..43]);

        assert!(f.replies().is_empty());
        assert!(f.world.0.borrow().speakers.is_empty());
    }

    #[test]
    fn a_pipeline_that_cannot_be_built_opens_nothing() {
        let mut f = Fixture::new();
        f.world.0.borrow_mut().refuse_speaker = true;

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        assert!(f.replies().is_empty());
    }

    #[test]
    fn a_stream_that_cannot_listen_is_not_announced() {
        let mut f = Fixture::new();
        f.world.0.borrow_mut().audio_deaf = true;

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        assert!(f.replies().is_empty());
    }

    #[test]
    fn packets_reach_the_pipeline_and_the_start_is_reported() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_ACTIVE, MUSIC, &[1]);
        f.audio_sink(0).borrow_mut().on_started(4242);
        f.audio_sink(0).borrow_mut().on_rtp(&[1, 2, 3], 4242);

        assert_eq!(f.speaker(0).pushed(), vec![vec![1, 2, 3]]);
        assert_eq!(
            f.replies().last(),
            Some(&(REPLY_AUDIO_STARTED, MUSIC, 4242u32.to_le_bytes().to_vec()))
        );
    }

    #[test]
    fn a_stream_stays_silent_until_it_is_made_active() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.audio_sink(0).borrow_mut().on_rtp(&[1, 2, 3], 0);

        assert!(f.speaker(0).pushed().is_empty());
    }

    #[test]
    fn an_active_stream_reaches_the_pipeline_and_a_held_one_stops_again() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_ACTIVE, MUSIC, &[1]);
        f.audio_sink(0).borrow_mut().on_rtp(&[1], 0);
        f.send(OP_AUDIO_ACTIVE, MUSIC, &[0]);
        f.audio_sink(0).borrow_mut().on_rtp(&[2], 0);

        assert_eq!(f.speaker(0).pushed(), vec![vec![1]]);
    }

    fn fed_body() -> Vec<u8> {
        let mut v = audio_body(3, false, "");
        v[11] |= 2;
        v
    }

    #[test]
    fn a_fed_stream_binds_no_ports_and_still_answers() {
        let mut f = Fixture::new();

        f.send(OP_AUDIO_OPEN, MUSIC, &fed_body());

        assert!(f.world.0.borrow().audio_sinks.is_empty());
        assert_eq!(f.replies(), vec![(REPLY_AUDIO_PORTS, MUSIC, vec![0, 0, 0, 0])]);
    }

    #[test]
    fn samples_handed_over_reach_the_pipeline_once_the_stream_is_active() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &fed_body());

        f.send(OP_AUDIO_DATA, MUSIC, &[1, 2]);
        f.send(OP_AUDIO_ACTIVE, MUSIC, &[1]);
        f.send(OP_AUDIO_DATA, MUSIC, &[3, 4]);

        assert_eq!(f.speaker(0).pushed(), vec![vec![3, 4]]);
    }

    #[test]
    fn the_volume_reaches_the_pipeline() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_VOLUME, MUSIC, &0.25f64.to_le_bytes());

        assert_eq!(f.speaker(0).volume(), Some(0.25));
    }

    #[test]
    fn a_volume_message_can_carry_a_ramp() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        let mut body = 0.5f64.to_le_bytes().to_vec();
        body.extend_from_slice(&250u32.to_le_bytes());
        f.send(OP_AUDIO_VOLUME, MUSIC, &body);

        assert_eq!(f.speaker(0).volume(), Some(0.5));
        assert_eq!(f.speaker(0).ramp_ms(), Some(250));
    }

    #[test]
    fn a_volume_message_short_of_a_value_is_ignored() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_VOLUME, MUSIC, &0.25f64.to_le_bytes()[..7]);

        assert_eq!(f.speaker(0).volume(), None);
    }

    #[test]
    fn stopping_an_audio_stream_drops_it() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_STOP, MUSIC, &[]);
        f.send(OP_AUDIO_VOLUME, MUSIC, &0.5f64.to_le_bytes());

        assert_eq!(f.speaker(0).volume(), None);
    }

    #[test]
    fn a_new_output_gives_the_stream_a_new_pipeline_at_its_level() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, "speakers"));
        f.send(OP_AUDIO_VOLUME, MUSIC, &0.25f64.to_le_bytes());
        f.send(OP_AUDIO_ACTIVE, MUSIC, &[1]);

        f.send(OP_AUDIO_OUTPUT, 0, b"headphones");
        f.audio_sink(0).borrow_mut().on_rtp(&[7], 0);

        assert_eq!(f.world.0.borrow().audio_cfgs[1].6.as_deref(), Some("headphones"));
        assert_eq!((f.speaker(1).volume(), f.speaker(1).ramp_ms()), (Some(0.25), Some(0)));
        assert_eq!(f.speaker(1).pushed(), vec![vec![7]]);
        assert!(f.speaker(0).pushed().is_empty());
    }

    #[test]
    fn a_stream_already_on_the_output_keeps_its_pipeline() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        f.send(OP_AUDIO_OUTPUT, 0, b"");

        assert_eq!(f.world.0.borrow().speakers.len(), 1);
    }

    #[test]
    fn a_stream_the_new_output_refuses_plays_on_where_it_was() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &fed_body());
        f.send(OP_AUDIO_ACTIVE, MUSIC, &[1]);

        f.world.0.borrow_mut().refuse_speaker = true;
        f.send(OP_AUDIO_OUTPUT, 0, b"gone");
        f.world.0.borrow_mut().refuse_speaker = false;
        f.send(OP_AUDIO_DATA, MUSIC, &[1]);

        assert_eq!(f.world.0.borrow().speakers.len(), 1);
        assert_eq!(f.speaker(0).pushed(), vec![vec![1]]);
    }

    fn mic_body(codec: u8, phone: &str, device: &str) -> Vec<u8> {
        let mut v = vec![codec, 97];
        v.extend_from_slice(&24000u32.to_le_bytes());
        v.push(1);
        v.extend_from_slice(&48000u32.to_le_bytes());
        v.extend_from_slice(&20u32.to_le_bytes());
        v.extend_from_slice(&5010u16.to_le_bytes());
        v.extend_from_slice(&[7u8; 32]);
        v.push(phone.len() as u8);
        v.extend_from_slice(phone.as_bytes());
        v.extend_from_slice(device.as_bytes());
        v
    }

    #[test]
    fn enabling_the_visualizer_turns_the_tap_on_for_every_stream() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));
        f.send(OP_AUDIO_OPEN, MUSIC + 1, &audio_body(0, false, ""));

        f.send(OP_VISUALIZER, 0, &[1]);

        assert_eq!(f.speaker(0).visualizer_on(), Some(true));
        assert_eq!(f.speaker(1).visualizer_on(), Some(true));
    }

    #[test]
    fn a_stream_opened_while_the_visualizer_is_on_taps_at_once() {
        let mut f = Fixture::new();
        f.send(OP_VISUALIZER, 0, &[1]);

        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));

        assert_eq!(f.speaker(0).visualizer_on(), Some(true));
    }

    #[test]
    fn pump_sends_the_bands_of_every_stream_summed_while_on() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));
        f.send(OP_AUDIO_OPEN, MUSIC + 1, &audio_body(0, false, ""));
        f.send(OP_VISUALIZER, 0, &[1]);
        let loud: Vec<u8> = (0..8192)
            .flat_map(|i| {
                let phase = 2.0 * std::f32::consts::PI * 22_000.0 * i as f32 / 48_000.0;
                ((phase.sin() * 16000.0) as i16).to_le_bytes()
            })
            .collect();
        f.speaker(0).feed_visualizer(&loud);
        f.speaker(1).feed_visualizer(&loud);

        f.host.pump_visualizer();

        let spectra: Vec<_> =
            f.replies().into_iter().filter(|(op, _, _)| *op == REPLY_VISUALIZER).collect();
        assert_eq!(spectra.len(), 1);
        let (_, id, bands) = &spectra[0];
        assert_eq!((*id, bands.len()), (0, spectrum::BANDS * 4));
        let top = f32::from_le_bytes([bands[92], bands[93], bands[94], bands[95]]);
        assert!(top > 0.5);
    }

    #[test]
    fn pump_stays_silent_while_the_visualizer_is_off() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));
        f.speaker(0).feed_visualizer(&[1, 2]);

        f.host.pump_visualizer();

        assert!(f.replies().iter().all(|(op, _, _)| *op != REPLY_VISUALIZER));
    }

    #[test]
    fn a_quiet_tap_sends_one_empty_frame_and_then_nothing() {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));
        f.send(OP_VISUALIZER, 0, &[1]);

        f.host.pump_visualizer();
        f.host.pump_visualizer();

        let spectra: Vec<_> =
            f.replies().into_iter().filter(|(op, _, _)| *op == REPLY_VISUALIZER).collect();
        assert_eq!(spectra, [(REPLY_VISUALIZER, 0, vec![0u8; spectrum::BANDS * 4])]);
    }

    const MIC: u32 = 0x7b00_0009;

    #[test]
    fn opening_the_microphone_carries_the_settings_and_the_phone_address() {
        let mut f = Fixture::new();

        f.send(OP_MIC_OPEN, MIC, &mic_body(0, "fe80::1", "hw:0"));

        assert_eq!(
            f.world.0.borrow().uplinks,
            vec![(97, 24000, 1, 48000, 20, 5010, "fe80::1".to_owned(), Some("hw:0".to_owned()))]
        );
    }

    #[test]
    fn a_microphone_message_short_of_its_address_opens_nothing() {
        let mut f = Fixture::new();

        f.send(OP_MIC_OPEN, MIC, &mic_body(0, "fe80::1", "")[..52]);

        assert!(f.world.0.borrow().uplinks.is_empty());
    }

    #[test]
    fn a_capture_chain_that_cannot_be_built_opens_nothing() {
        let mut f = Fixture::new();
        f.world.0.borrow_mut().refuse_uplink = true;

        f.send(OP_MIC_OPEN, MIC, &mic_body(0, "fe80::1", ""));

        assert!(f.world.0.borrow().uplinks.is_empty());
    }

    #[test]
    fn stopping_the_microphone_drops_the_capture() {
        let mut f = Fixture::new();
        f.send(OP_MIC_OPEN, MIC, &mic_body(0, "fe80::1", ""));

        f.send(OP_MIC_STOP, MIC, &[]);

        assert_eq!(*f.world.0.borrow().uplinks_dropped.borrow(), 1);
    }

    #[test]
    fn the_statistics_name_the_plane_and_start_over_when_read() {
        let mut f = Fixture::new();
        f.feeder(42, MAIN_PLANE, false);
        f.config(0, CpCodec::H264, &[1, 2]);
        f.frame_in(0, &nal(DELTA, 1));

        let first = f.host.take_stats();

        assert_eq!(first.len(), 1);
        assert!(first[0].contains("recv 0x7a000001: in=1 dropped=1 pushed=0"));
        assert!(first[0].ends_with("awaiting_kf=1 active=1"));
        assert!(f.host.take_stats().is_empty());
    }
    fn tap_body(rate: u32, channels: u8, device: &str, path: &str) -> Vec<u8> {
        let mut b = rate.to_le_bytes().to_vec();
        b.push(channels);
        b.push(device.len() as u8);
        b.extend_from_slice(device.as_bytes());
        b.extend_from_slice(path.as_bytes());
        b
    }

    #[test]
    fn a_tap_message_opens_a_capture_to_the_path_and_stop_drops_it() {
        let mut f = Fixture::new();
        f.send(OP_TAP_OPEN, 0x7c01, &tap_body(16000, 1, "mic0", "/tmp/aa.mic"));
        assert_eq!(
            f.world.0.borrow().taps,
            vec![(16000, 1, Some("mic0".to_owned()), "/tmp/aa.mic".to_owned())]
        );

        f.send(OP_TAP_STOP, 0x7c01, &[]);

        assert_eq!(*f.world.0.borrow().taps_dropped.borrow(), 1);
    }

    #[test]
    fn a_tap_message_short_of_its_device_opens_nothing() {
        let mut f = Fixture::new();
        f.send(OP_TAP_OPEN, 0x7c01, &[0, 0, 0, 0, 1, 9]);
        assert!(f.world.0.borrow().taps.is_empty());
    }
}
