// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

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
    output_delay_ms: Option<u32>,
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

    fn set_output_delay(&self, ms: u32) {
        self.0.lock().unwrap().output_delay_ms = Some(ms);
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

    fn output_delay_ms(&self) -> Option<u32> {
        self.0.lock().unwrap().output_delay_ms
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
    audio_codec_data: Vec<Vec<u8>>,
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
    urls: Vec<(u32, String, String)>,
    url_logs: Vec<Rc<RefCell<UrlLog>>>,
    refuse_url: bool,
    cameras: Vec<(u32, CameraOrder)>,
    camera_watches: usize,
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
        w.taps
            .push((cfg.sample_rate, cfg.channels, cfg.device, cfg.path));
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

    type Camera = ();

    fn open_camera(&self, id: u32, order: &CameraOrder) -> Option<()> {
        self.0.borrow_mut().cameras.push((id, order.clone()));
        Some(())
    }

    fn watch_cameras(&self, report: Box<dyn Fn(Vec<CameraSeen>) + Send>) {
        self.0.borrow_mut().camera_watches += 1;
        report(vec![CameraSeen {
            id: "/dev/video0".into(),
            name: "USB Cam".into(),
        }]);
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
        w.audio_codec_data.push(cfg.codec_data.clone());
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
type OpenedUplink = (u8, u32, u8, u32, u32, u16, String, Option<String>);
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
                "[feed] recv 0x7a000001: in=1 dropped=1 pushed=0 awaiting_kf=1 active=1".to_owned()
            ]
        );
        assert!(f.host.take_stats().is_empty());
    }

    #[test]
    fn opening_the_feed_answers_with_the_path() {
        let mut f = Fixture::new();
        f.open_feed("/tmp/x.feed");
        assert_eq!(f.replies(), vec![(REPLY_FEED, 9, b"/tmp/x.feed".to_vec())]);
        assert_eq!(
            f.world.0.borrow().feed_paths,
            vec!["/tmp/x.feed".to_owned()]
        );
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

        assert_eq!(
            f.plane(0).pushed(),
            vec![nal(KEYFRAME, 1), nal(KEYFRAME, 3)]
        );
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
        assert!(
            stats
                .iter()
                .any(|l| l.starts_with("[feed] recv 0x7b000002 -> 0x7a000001:"))
        );
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

        assert_eq!(
            f.plane(0).pushed(),
            vec![nal(KEYFRAME, 2), nal(KEYFRAME, 4)]
        );
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

    assert_eq!(
        f.world.0.borrow().codecs,
        vec![(MAIN_PLANE, "h264".to_owned(), vec![9, 9])]
    );
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
fn a_camera_order_opens_the_camera_and_stop_ends_it() {
    let mut f = Fixture::new();
    let order = CameraOrder {
        device: "/dev/video0".into(),
        rotation: 90,
        mirror: true,
    };
    f.send(
        OP_CAMERA_OPEN,
        livi_host_proto::PLANE_CAMERA,
        &livi_host_proto::camera_body(&order),
    );
    assert_eq!(
        f.world.0.borrow().cameras,
        vec![(livi_host_proto::PLANE_CAMERA, order)]
    );
    assert_eq!(f.host.cameras.len(), 1);
    f.send(OP_STOP, livi_host_proto::PLANE_CAMERA, &[]);
    assert!(f.host.cameras.is_empty());
}

#[test]
fn the_camera_list_is_watched_once_and_reported() {
    let mut f = Fixture::new();
    f.send(OP_CAMERAS_WATCH, 0, &[]);
    f.send(OP_CAMERAS_WATCH, 0, &[]);
    assert_eq!(f.world.0.borrow().camera_watches, 1);
    let sent = f.sent.0.lock().unwrap();
    let reports: Vec<_> = sent
        .iter()
        .filter(|(op, ..)| *op == REPLY_CAMERAS)
        .collect();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        livi_host_proto::parse_cameras_body(&reports[0].2)[0].name,
        "USB Cam"
    );
}

#[test]
fn a_play_message_starts_a_player_on_its_url_and_audio_device() {
    let mut f = Fixture::new();

    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/i.m3u8", "car"),
    );

    assert_eq!(
        f.world.0.borrow().urls,
        vec![(VIDEO_PLANE, "http://h/i.m3u8".to_owned(), "car".to_owned())]
    );
    assert_eq!(url_log(&f, 0).borrow().started, 1);
}

#[test]
fn rate_seek_and_mute_reach_the_player() {
    let mut f = Fixture::new();
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );

    f.send(OP_PLAY_RATE, VIDEO_PLANE, &0.0f64.to_le_bytes());
    f.send(OP_PLAY_SEEK, VIDEO_PLANE, &12.5f64.to_le_bytes());
    f.send(OP_PLAY_MUTE, VIDEO_PLANE, &[1]);

    let log = url_log(&f, 0);
    let log = log.borrow();
    assert_eq!(
        (log.rates.as_slice(), log.seeks.as_slice()),
        (&[0.0][..], &[12.5][..])
    );
    assert_eq!(log.muted, Some(true));
}

#[test]
fn control_messages_short_of_their_value_or_their_player_are_ignored() {
    let mut f = Fixture::new();
    f.send(OP_PLAY_RATE, VIDEO_PLANE, &1.0f64.to_le_bytes());
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );

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
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );
    let status = UrlStatus {
        position: Some(4.0),
        ready: true,
        playing: true,
        ..Default::default()
    };
    url_log(&f, 0).borrow_mut().status = status;

    f.host.pump_url_status();

    assert_eq!(
        f.replies(),
        vec![(REPLY_URL_STATUS, VIDEO_PLANE, url_status_body(&status))]
    );
}

#[test]
fn a_stop_message_drops_the_player() {
    let mut f = Fixture::new();
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );

    f.send(OP_STOP, VIDEO_PLANE, &[]);
    f.send(OP_PLAY_MUTE, VIDEO_PLANE, &[1]);

    assert!(url_log(&f, 0).borrow().dropped);
    assert_eq!(url_log(&f, 0).borrow().muted, None);
}

#[test]
fn a_player_and_a_plane_on_one_id_replace_each_other() {
    let mut f = Fixture::new();
    f.send(OP_CREATE, VIDEO_PLANE, &create_body("h264", &[]));

    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );
    f.send(OP_DATA, VIDEO_PLANE, &[7]);
    assert!(f.plane(0).pushed().is_empty());

    f.send(OP_CREATE, VIDEO_PLANE, &create_body("h264", &[]));
    assert!(url_log(&f, 0).borrow().dropped);
}

#[test]
fn a_play_message_without_a_url_or_a_player_leaves_nothing_behind() {
    let mut f = Fixture::new();
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("", "car"),
    );
    f.world.0.borrow_mut().refuse_url = true;
    f.send(
        OP_PLAY_URL,
        VIDEO_PLANE,
        &livi_host_proto::url_body("http://h/", ""),
    );

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

    f.send(
        OP_GAMMA,
        MAIN_PLANE,
        &gamma_body([1.5, 2.0, 0.5, 0.6, 0.7])[..39],
    );

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

    assert_eq!(
        f.replies(),
        vec![(REPLY_PORT, 42, 5555u16.to_le_bytes().to_vec())]
    );
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

    assert_eq!(
        f.replies().last(),
        Some(&(REPLY_STARTED, MAIN_PLANE, vec![]))
    );
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

    f.send(
        OP_AUDIO_OPEN,
        MUSIC,
        &audio_body(1, true, "alsa_output.front"),
    );

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
    for (byte, expected) in [
        (0u8, AudioCodec::AacLc),
        (1, AudioCodec::Opus),
        (2, AudioCodec::Lpcm),
        (4, AudioCodec::AacRaw),
    ] {
        let mut f = Fixture::new();
        f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(byte, false, ""));

        assert_eq!(f.world.0.borrow().audio_cfgs[0].0, expected);
    }
}

#[test]
fn an_aac_raw_stream_takes_its_config_from_the_key_slot() {
    let mut f = Fixture::new();
    let mut body = audio_body(4, false, "");
    body[12..17].copy_from_slice(&[4, 0x13, 0x14, 0x56, 0xe5]);

    f.send(OP_AUDIO_OPEN, MUSIC, &body);

    assert_eq!(
        f.world.0.borrow().audio_codec_data,
        vec![vec![0x13, 0x14, 0x56, 0xe5]]
    );
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
    assert_eq!(
        f.replies(),
        vec![(REPLY_AUDIO_PORTS, MUSIC, vec![0, 0, 0, 0])]
    );
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

    assert_eq!(
        f.world.0.borrow().audio_cfgs[1].6.as_deref(),
        Some("headphones")
    );
    assert_eq!(
        (f.speaker(1).volume(), f.speaker(1).ramp_ms()),
        (Some(0.25), Some(0))
    );
    assert_eq!(f.speaker(1).pushed(), vec![vec![7]]);
    assert!(f.speaker(0).pushed().is_empty());
}

#[test]
fn a_stream_tells_its_output_delay_once_it_changes() {
    let mut f = Fixture::new();
    f.send(OP_AUDIO_OPEN, MUSIC, &audio_body(0, false, ""));
    let delays = |f: &Fixture| -> Vec<u32> {
        f.replies()
            .into_iter()
            .filter(|(op, id, _)| *op == REPLY_AUDIO_DELAY && *id == MUSIC)
            .map(|(_, _, rest)| u32::from_le_bytes(rest[..4].try_into().unwrap()))
            .collect()
    };

    f.host.pump_audio_delays();
    assert!(delays(&f).is_empty(), "nothing plays yet");
    f.speaker(0).set_output_delay(40);
    f.host.pump_audio_delays();
    f.speaker(0).set_output_delay(80);
    f.host.pump_audio_delays();
    assert_eq!(delays(&f), [40], "one jump is smoothed below the step");
    for _ in 0..20 {
        f.host.pump_audio_delays();
    }
    let told = delays(&f);
    assert!(
        told.len() >= 2 && *told.last().unwrap() >= 70,
        "a lasting change gets through: {told:?}"
    );
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

    let spectra: Vec<_> = f
        .replies()
        .into_iter()
        .filter(|(op, _, _)| *op == REPLY_VISUALIZER)
        .collect();
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

    let spectra: Vec<_> = f
        .replies()
        .into_iter()
        .filter(|(op, _, _)| *op == REPLY_VISUALIZER)
        .collect();
    assert_eq!(
        spectra,
        [(REPLY_VISUALIZER, 0, vec![0u8; spectrum::BANDS * 4])]
    );
}

const MIC: u32 = 0x7b00_0009;

#[test]
fn opening_the_microphone_carries_the_settings_and_the_phone_address() {
    let mut f = Fixture::new();

    f.send(OP_MIC_OPEN, MIC, &mic_body(0, "fe80::1", "hw:0"));

    assert_eq!(
        f.world.0.borrow().uplinks,
        vec![(
            97,
            24000,
            1,
            48000,
            20,
            5010,
            "fe80::1".to_owned(),
            Some("hw:0".to_owned())
        )]
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
    f.send(
        OP_TAP_OPEN,
        0x7c01,
        &tap_body(16000, 1, "mic0", "/tmp/aa.mic"),
    );
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
