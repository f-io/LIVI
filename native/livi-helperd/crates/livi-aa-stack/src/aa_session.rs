use std::sync::Arc;

use serde_json::{Map, Value, json};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{Instant, Interval, MissedTickBehavior};

use crate::bridge::{Bridge, BridgeOut, Report};
use crate::channels::audio::AudioChannelType;
use crate::commands::{
    Action, Command, InputCommand, TouchAction, TouchItem, command_action, input_key, multi_touch,
    single_touch_action, touch_point,
};
use crate::config::AaConfig;
use crate::consts::ch;
use crate::discovery::VideoCodec;
use crate::link::{Link, LinkItem, LinkReader};
use crate::log::detail;
use crate::manager::{AaEvent, SessionId};
use crate::media::{AaMedia, AudioKind, AudioOutput};
use crate::sensors::Sensor;
use crate::session::{BYEBYE_USER_SELECTION, Out, Session, SessionEvent, Timer};

#[derive(Debug, Clone, PartialEq)]
pub enum SessionCmd {
    /// 0 to 1 across the main screen.
    Touch {
        action: TouchAction,
        x: f64,
        y: f64,
    },
    MultiTouch(Vec<TouchItem>),
    Command(Command),
    Input(InputCommand),
    Disconnect,
    Keyframe,
    ClusterActive(bool),
    /// Covers audio too.
    VideoActive(bool),
    StreamVolume {
        kind: AudioKind,
        level: f64,
        ramp_ms: u32,
    },
    Sensor(Sensor),
    Close,
}

enum Internal {
    Sink { video: bool, fields: Map<String, Value> },
}

struct Phone<M: AaMedia> {
    id: SessionId,
    session: Session,
    bridge: Bridge,
    link: Link,
    link_open: bool,
    media: Arc<M>,
    config: watch::Receiver<AaConfig>,
    events: mpsc::UnboundedSender<AaEvent>,
    internal: mpsc::UnboundedSender<Internal>,
    timers: Vec<(Instant, Timer)>,
    ping: Option<Interval>,
    outputs: Option<broadcast::Receiver<AudioOutput>>,
    closed: bool,
    session_up: bool,
    down_emitted: bool,
    mic_tap: Option<u32>,
    mic_active: bool,
    close_pending: bool,
    closing: bool,
    shutdown_done: bool,
}

async fn tick(ping: &mut Option<Interval>) {
    match ping {
        Some(i) => {
            i.tick().await;
        }
        None => std::future::pending().await,
    }
}

async fn output(
    rx: &mut Option<broadcast::Receiver<AudioOutput>>,
) -> Result<AudioOutput, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn read(reader: &mut LinkReader, open: bool) -> LinkItem {
    if open { reader.next().await } else { std::future::pending().await }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run<M: AaMedia>(
    id: SessionId,
    link: Link,
    mut reader: LinkReader,
    config: watch::Receiver<AaConfig>,
    media: Arc<M>,
    cluster_stream_active: bool,
    mut cmds: mpsc::UnboundedReceiver<SessionCmd>,
    events: mpsc::UnboundedSender<AaEvent>,
) {
    let (internal, mut internal_rx) = mpsc::unbounded_channel();
    let (session, first) = Session::new(config.clone(), &link.peer);
    let outputs = Some(media.audio_output_opened());
    let mut p = Phone {
        id,
        session,
        bridge: Bridge::default(),
        link,
        link_open: true,
        media,
        config,
        events,
        internal,
        timers: Vec::new(),
        ping: None,
        outputs,
        closed: false,
        session_up: false,
        down_emitted: false,
        mic_tap: None,
        mic_active: false,
        close_pending: false,
        closing: false,
        shutdown_done: false,
    };
    p.apply(first);
    let seeded = p.session.set_cluster_stream_active(cluster_stream_active);
    p.apply(seeded);

    let mut commands_open = true;
    loop {
        let deadline = p.timers.iter().map(|(at, _)| *at).min();
        tokio::select! {
            item = read(&mut reader, p.link_open) => p.on_link(item),
            cmd = cmds.recv(), if commands_open => match cmd {
                Some(cmd) => p.on_cmd(cmd),
                None => {
                    commands_open = false;
                    p.begin_close();
                }
            },
            () = tick(&mut p.ping) => {
                let out = p.session.on_ping_tick();
                p.apply(out);
            }
            () = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                p.fire_timers();
            }
            got = output(&mut p.outputs) => match got {
                Ok(o) => p.push_audio_sink(&o),
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => p.outputs = None,
            },
            Some(msg) = internal_rx.recv() => p.on_internal(msg),
        }
        if p.close_pending {
            p.close_pending = false;
            p.begin_close();
        }
        if p.closing && p.shutdown_done {
            let out = p.session.close("stack restart");
            p.apply(out);
            break;
        }
    }
}

impl<M: AaMedia> Phone<M> {
    fn emit(&self, event: AaEvent) {
        if self.down_emitted && !matches!(event, AaEvent::Disconnected { .. }) {
            return;
        }
        let _ = self.events.send(event);
    }

    fn apply(&mut self, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Send(frame) => self.link.send(&frame),
                Out::Control(v) => self.link.control(&v),
                Out::End => self.link.end(),
                Out::Destroy => self.link.destroy(),
                Out::Event(e) => self.on_session_event(e),
                Out::Ping(Some(every)) => {
                    let mut i = tokio::time::interval_at(Instant::now() + every, every);
                    i.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    self.ping = Some(i);
                }
                Out::Ping(None) => self.ping = None,
                Out::After(d, t) => self.timers.push((Instant::now() + d, t)),
                Out::ShutdownDone => self.shutdown_done = true,
            }
        }
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = self.timers.drain(..).partition(|(at, _)| *at <= now);
        self.timers = later;
        for (_, t) in due {
            let out = self.session.on_timer(t);
            self.apply(out);
        }
    }

    fn on_link(&mut self, item: LinkItem) {
        let out = match item {
            LinkItem::Message(f) => self.session.on_message(f.ch, f.msg_id, &f.payload),
            LinkItem::Control(v) => self.session.on_link_control(&v),
            LinkItem::Closed(err) => {
                self.link_open = false;
                self.link.mark_closed();
                self.session.on_link_closed(err)
            }
        };
        self.apply(out);
    }

    fn on_session_event(&mut self, event: SessionEvent) {
        let cfg = self.session.config();
        for b in self.bridge.on_event(event, &cfg) {
            self.on_bridge(b);
        }
    }

    fn on_bridge(&mut self, b: BridgeOut) {
        let id = self.id;
        match b {
            BridgeOut::Report(Report::Connected) => {
                if !self.session_up {
                    self.session_up = true;
                    self.emit(AaEvent::Connected { id });
                }
            }
            BridgeOut::Report(Report::Disconnected) => {
                self.emit_down();
                self.outputs = None;
                self.close_pending = true;
            }
            BridgeOut::Report(r) => self.emit(AaEvent::from_report(id, r)),
            BridgeOut::StartMic(reason) => self.start_mic(reason),
            BridgeOut::StopMic(reason) => self.stop_mic(reason),
            BridgeOut::VideoSink { cluster, codec } => self.push_video_sink(cluster, codec),
            BridgeOut::AudioSinks => {
                for o in self.media.audio_outputs() {
                    self.push_audio_sink(&o);
                }
            }
            BridgeOut::PrimeAudio { kind, sample_rate, channels, tag } => {
                self.media.prime_audio(kind, sample_rate, channels, tag.name());
            }
            BridgeOut::VideoStarted { cluster, width, height } => {
                self.media.video_started(cluster, width, height);
            }
        }
    }

    /// Core hears the end once and last, after the main focus was given back
    /// and the microphone stopped.
    fn emit_down(&mut self) {
        if self.down_emitted {
            return;
        }
        self.session_up = false;
        for b in self.bridge.reset() {
            self.on_bridge(b);
        }
        self.mic_active = false;
        self.drop_mic_tap();
        self.media.release(self.id);
        self.down_emitted = true;
        self.emit(AaEvent::Disconnected { id: self.id });
    }

    fn begin_close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.emit_down();
        self.closing = true;
        self.shutdown_done = false;
        let out = self.session.request_shutdown(BYEBYE_USER_SELECTION);
        self.apply(out);
    }

    fn on_cmd(&mut self, cmd: SessionCmd) {
        let out = match cmd {
            SessionCmd::Touch { action, x, y } => {
                let g = self.session.geometry();
                match touch_point(&g, 0, x, y) {
                    Some(p) => self.session.send_touch(single_touch_action(action), &[p], 0),
                    None => Vec::new(),
                }
            }
            SessionCmd::MultiTouch(touches) => {
                let g = self.session.geometry();
                match multi_touch(&g, &touches) {
                    Some((action, pointers, index)) => {
                        self.session.send_touch(action, &pointers, index)
                    }
                    None => Vec::new(),
                }
            }
            SessionCmd::Command(c) => match command_action(c) {
                Action::Key { code, down } => self.session.send_button(&[code], down),
                Action::Click(code) => self.click(code),
                Action::Rotary(d) => self.session.send_rotary(d),
                Action::VideoFocus => self.session.request_video_focus(),
                Action::ClusterFocus => self.session.request_cluster_keyframe(),
                Action::Nothing => Vec::new(),
            },
            SessionCmd::Input(i) => self.click(input_key(i)),
            SessionCmd::Disconnect => self.session.request_shutdown(BYEBYE_USER_SELECTION),
            SessionCmd::Keyframe => {
                let mut out = self.session.request_main_keyframe();
                out.extend(self.session.force_cluster_keyframe());
                out
            }
            SessionCmd::ClusterActive(active) => self.session.set_cluster_stream_active(active),
            SessionCmd::VideoActive(active) => {
                self.media.set_active(self.id, active);
                Vec::new()
            }
            SessionCmd::StreamVolume { kind, level, ramp_ms } => {
                self.media.set_host_volume(kind, level, ramp_ms);
                Vec::new()
            }
            SessionCmd::Sensor(s) => self.session.send_sensor(&s),
            SessionCmd::Close => {
                self.begin_close();
                Vec::new()
            }
        };
        self.apply(out);
    }

    fn click(&mut self, code: u32) -> Vec<Out> {
        let mut out = self.session.send_button(&[code], true);
        out.extend(self.session.send_button(&[code], false));
        out
    }

    fn start_mic(&mut self, reason: &str) {
        if self.mic_active || self.down_emitted {
            return;
        }
        let Some(path) = self.session.mic_socket_path().map(str::to_string) else {
            eprintln!("[AaSession] {reason} -> no mic socket from the helper yet");
            return;
        };
        let (rate, channels) = self.session.mic_format();
        detail!("[AaSession] {reason} -> starting mic tap ({rate}Hz {channels}ch)");
        let device = self.config.borrow().mic_device.clone();
        self.mic_tap = self.media.open_mic_tap(&path, rate, channels, &device);
        self.mic_active = self.mic_tap.is_some();
        if self.mic_active {
            self.emit(AaEvent::Mic { id: self.id, active: true });
        }
    }

    fn stop_mic(&mut self, reason: &str) {
        if !self.mic_active {
            return;
        }
        self.mic_active = false;
        detail!("[AaSession] {reason} -> stopping mic tap");
        self.drop_mic_tap();
    }

    fn drop_mic_tap(&mut self) {
        if let Some(tap) = self.mic_tap.take() {
            self.media.close_mic_tap(tap);
            self.emit(AaEvent::Mic { id: self.id, active: false });
        }
    }

    /// The plane comes first, so the host has a decoder when the feed starts.
    fn push_video_sink(&mut self, cluster: bool, codec: VideoCodec) {
        self.media.prime_video(cluster, codec);
        let entry = json!({
            "ch": if cluster { ch::CLUSTER_VIDEO } else { ch::VIDEO },
            "id": self.media.video_feed(self.id, cluster),
            "codec": codec.name(),
        });
        self.deliver(true, "video", entry);
    }

    fn push_audio_sink(&mut self, o: &AudioOutput) {
        let Some(channel) = o.tag.as_deref().and_then(AudioChannelType::from_name) else { return };
        let id = self.media.audio_feed(self.id, o.stream);
        self.deliver(false, "audio", json!({ "ch": channel.channel(), "id": id }));
    }

    fn deliver(&self, video: bool, key: &'static str, entry: Value) {
        let media = self.media.clone();
        let tx = self.internal.clone();
        tokio::spawn(async move {
            let feed = media.feed_path().await;
            let mut fields = Map::new();
            fields.insert("feed".into(), Value::String(feed));
            fields.insert(key.into(), Value::Array(vec![entry]));
            let _ = tx.send(Internal::Sink { video, fields });
        });
    }

    fn on_internal(&mut self, msg: Internal) {
        match msg {
            Internal::Sink { video, fields } => {
                if self.closed {
                    return;
                }
                if video && fields.get("feed").and_then(Value::as_str) == Some("") {
                    eprintln!("[AaEventBridge] host has no media feed, video will not show");
                }
                let out = self.session.send_media_sink(fields);
                self.apply(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::UnixStream;

    use super::*;
    use crate::bridge::{NowPlaying, PhoneStatus};
    use crate::channels::Frame;
    use crate::link;
    use crate::media::tests::FakeMedia;
    use crate::session::{CallState, PhoneCall};

    struct Rig {
        helper: Link,
        from: LinkReader,
        cmds: mpsc::UnboundedSender<SessionCmd>,
        events: mpsc::UnboundedReceiver<AaEvent>,
        media: Arc<FakeMedia>,
        _cfg: watch::Sender<AaConfig>,
    }

    fn rig(cfg: AaConfig, mic_refused: bool) -> Rig {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (link, reader) = link::attach(ours, "10.0.0.7");
        let (helper, from) = link::attach(theirs, "");
        let (cfg_tx, cfg_rx) = watch::channel(cfg);
        let media = FakeMedia::new();
        media.refuse_mic.store(mic_refused, std::sync::atomic::Ordering::Relaxed);
        let (cmds, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, events) = mpsc::unbounded_channel();
        tokio::spawn(run(7, link, reader, cfg_rx, media.clone(), true, cmd_rx, ev_tx));
        Rig { helper, from, cmds, events, media, _cfg: cfg_tx }
    }

    impl Rig {
        fn phone(&self, ch: u8, msg_id: u16, payload: &[u8]) {
            self.helper.send(&Frame::new(ch, 0x0b, msg_id, payload.to_vec()));
        }

        async fn event(&mut self) -> AaEvent {
            tokio::time::timeout(Duration::from_secs(5), self.events.recv())
                .await
                .expect("an event in time")
                .expect("the session runs")
        }

        async fn item(&mut self) -> LinkItem {
            tokio::time::timeout(Duration::from_secs(5), self.from.next()).await.expect("in time")
        }

        /// Skips pings, message 0x000b on channel 0.
        async fn frame(&mut self) -> Frame {
            loop {
                match self.item().await {
                    LinkItem::Message(f) if f.ch == 0 && f.msg_id == 0x000b => {}
                    LinkItem::Message(f) => return f,
                    LinkItem::Control(_) => {}
                    LinkItem::Closed(e) => panic!("closed {e:?}"),
                }
            }
        }

        async fn control(&mut self) -> Value {
            loop {
                if let LinkItem::Control(v) = self.item().await {
                    return v;
                }
            }
        }

        async fn running(&mut self, mic: Option<&str>) {
            let mut ready = json!({ "type": "ready" });
            if let Some(mic) = mic {
                ready["mic"] = json!(mic);
            }
            self.helper.control(&ready);
            let sdr = [0x22, 0x01, b'P', 0x2a, 0x01, b'G', 0x32, 0x03, 0x0a, 0x01, b'I'];
            self.phone(0, 0x0005, &sdr);
            assert_eq!(
                self.event().await,
                AaEvent::Device {
                    id: 7,
                    name: "P".into(),
                    model: "G".into(),
                    instance_id: "I".into(),
                    ip: "10.0.0.7".into()
                }
            );
            self.phone(3, 0x8000, &[0x08, 0x07]);
            assert_eq!(
                self.event().await,
                AaEvent::VideoCodec { id: 7, cluster: false, codec: VideoCodec::H265 }
            );
            assert_eq!(self.event().await, AaEvent::Connected { id: 7 });
            assert_eq!(
                self.control().await,
                json!({ "type": "sink", "feed": "/tmp/gst.feed", "video": [{ "ch": 3, "id": 1, "codec": "h265" }] })
            );
        }
    }

    fn hevc() -> AaConfig {
        AaConfig { hevc_supported: true, mic_device: "usb".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn a_session_reports_in_livi_terms() {
        let mut r = rig(hevc(), false);
        r.running(Some("/tmp/m.sock")).await;
        assert_eq!(r.media.take(), [json!({ "primeVideo": false })]);

        r.helper.control(&json!({ "type": "first-frame", "ch": 3 }));
        assert_eq!(r.event().await, AaEvent::VideoFocus { id: 7, cluster: false, projected: true });
        r.phone(4, 0x8000, &[0x08, 0x01]);
        let setup = r.frame().await;
        assert_eq!((setup.ch, setup.msg_id), (4, 0x8003));
        r.media
            .opened
            .send(AudioOutput { kind: AudioKind::Media, stream: 41, tag: Some("media".into()) })
            .unwrap();
        assert_eq!(
            r.control().await,
            json!({ "type": "sink", "feed": "/tmp/gst.feed", "audio": [{ "ch": 4, "id": 41 }] })
        );
        assert_eq!(
            r.media.take(),
            [
                json!({ "videoStarted": [false, 1280, 720] }),
                json!({ "primeAudio": [3, 48000, 2, "media"] })
            ]
        );
        r.phone(4, 0x8001, &[0x08, 0x01]);
        assert_eq!(
            r.event().await,
            AaEvent::Audio {
                id: 7,
                stream: AudioChannelType::Media,
                sample_rate: 48000,
                channels: 2,
                active: true
            }
        );
        r.phone(0, 0x0012, &[0x08, 0x03]);
        assert_eq!(r.event().await, AaEvent::Duck { id: 7, level: 0.2, duration_ms: 500 });
        r.phone(0, 0x0011, &[0x08, 0x01]);
        assert_eq!(r.event().await, AaEvent::Mic { id: 7, active: true });
        r.phone(0, 0x0011, &[0x08, 0x02]);
        assert_eq!(r.event().await, AaEvent::Mic { id: 7, active: false });
        assert_eq!(
            r.media.take(),
            [
                json!({ "micTap": ["/tmp/m.sock", { "sampleRate": 16000, "channels": 1, "device": "usb" }] }),
                json!({ "micTapClose": true })
            ]
        );
        r.phone(13, 0x8003, &[0x0a, 0x01, b'S']);
        assert_eq!(
            r.event().await,
            AaEvent::NowPlaying {
                id: 7,
                now_playing: NowPlaying { title: Some("S".into()), ..Default::default() }
            }
        );
        r.phone(12, 0x8001, &[]);
        assert!(matches!(r.event().await, AaEvent::Navigation { id: 7, .. }));
        r.phone(14, 0x8001, &[0x0a, 0x04, 0x08, 0x04, 0x10, 0x05, 0x10, 0x03]);
        assert_eq!(
            r.event().await,
            AaEvent::Status {
                id: 7,
                status: PhoneStatus {
                    ip: "10.0.0.7".into(),
                    signal_strength: Some(3),
                    ..Default::default()
                }
            }
        );
        let AaEvent::Calls { calls, .. } = r.event().await else { panic!("no calls") };
        assert_eq!(
            calls,
            [PhoneCall {
                state: CallState::Incoming,
                duration_s: 5,
                number: None,
                caller_id: None,
                number_type: None,
                thumbnail: None
            }]
        );
        r.phone(3, 0x8007, &[0x10, 0x02]);
        assert_eq!(r.event().await, AaEvent::HostUiRequested { id: 7 });

        r.cmds.send(SessionCmd::VideoActive(false)).unwrap();
        r.cmds
            .send(SessionCmd::StreamVolume { kind: AudioKind::Alert, level: 0.5, ramp_ms: 100 })
            .unwrap();
        r.cmds.send(SessionCmd::Close).unwrap();
        assert_eq!(
            r.event().await,
            AaEvent::VideoFocus { id: 7, cluster: false, projected: false }
        );
        assert_eq!(r.event().await, AaEvent::Disconnected { id: 7 });
        assert_eq!(
            r.media.take(),
            [
                json!({ "active": [7, false] }),
                json!({ "volume": [4, 0.5, 100] }),
                json!({ "release": 7 })
            ]
        );
        loop {
            let f = r.frame().await;
            if f.msg_id == 0x000f {
                assert_eq!(f.payload, [0x08, 0x01]);
                break;
            }
        }
        r.phone(0, 0x0010, &[]);
        assert_eq!(r.control().await, json!({ "type": "end" }));
        assert_eq!(r.control().await, json!({ "type": "close" }));
        assert_eq!(r.item().await, LinkItem::Closed(None));
        assert_eq!(r.events.recv().await, None);
    }

    #[tokio::test]
    async fn the_phone_says_goodbye_and_hears_back() {
        let mut r = rig(hevc(), false);
        r.running(None).await;
        r.phone(0, 0x000f, &[0x08, 0x02]);
        loop {
            let f = r.frame().await;
            if f.msg_id == 0x0010 {
                assert!(f.payload.is_empty());
                break;
            }
        }
        assert_eq!(r.event().await, AaEvent::Disconnected { id: 7 });
        assert_eq!(r.control().await, json!({ "type": "close" }));
        assert_eq!(r.events.recv().await, None);
    }

    #[tokio::test]
    async fn the_microphone_needs_a_socket_and_a_capture() {
        let mut r = rig(hevc(), false);
        r.running(None).await;
        r.phone(0, 0x0011, &[0x08, 0x01]);
        r.phone(9, 0x8005, &[0x08, 0x01]);
        assert_eq!(r.frame().await.msg_id, 0x8006);
        assert!(r.media.take().iter().all(|e| e.get("micTap").is_none()));

        let mut r = rig(hevc(), true);
        r.running(Some("/tmp/m.sock")).await;
        r.phone(0, 0x0011, &[0x08, 0x01]);
        r.phone(9, 0x8005, &[0x08, 0x01]);
        assert_eq!(r.frame().await.msg_id, 0x8006);
        let taps: Vec<_> =
            r.media.take().into_iter().filter(|e| e.get("micTap").is_some()).collect();
        assert_eq!(taps.len(), 2);
        r.helper.destroy();
        assert_eq!(r.event().await, AaEvent::Disconnected { id: 7 });
    }

    #[tokio::test]
    async fn input_and_keyframes_reach_the_phone() {
        let mut r = rig(hevc(), false);
        r.running(None).await;
        r.cmds.send(SessionCmd::Touch { action: TouchAction::Down, x: 0.5, y: 0.5 }).unwrap();
        assert_eq!(r.frame().await.ch, ch::INPUT);
        r.cmds.send(SessionCmd::Command(Command::Up)).unwrap();
        assert_eq!(r.frame().await.ch, ch::INPUT);
        assert_eq!(r.frame().await.ch, ch::INPUT);
        r.cmds.send(SessionCmd::Input(InputCommand::Play)).unwrap();
        r.cmds.send(SessionCmd::Command(Command::Frame)).unwrap();
        r.cmds.send(SessionCmd::Command(Command::RequestHostUi)).unwrap();
        r.cmds.send(SessionCmd::Keyframe).unwrap();
        r.cmds.send(SessionCmd::ClusterActive(false)).unwrap();
        r.cmds
            .send(SessionCmd::MultiTouch(vec![TouchItem {
                id: 2,
                x: 0.1,
                y: 0.1,
                action: TouchAction::Up,
            }]))
            .unwrap();
        r.cmds.send(SessionCmd::Disconnect).unwrap();
        let mut seen = Vec::new();
        loop {
            let f = r.frame().await;
            seen.push((f.ch, f.msg_id, f.payload.clone()));
            if f.msg_id == 0x000f {
                break;
            }
        }
        assert!(seen.contains(&(ch::VIDEO, 0x8007, vec![0x10, 0x01, 0x18, 0x00])));
        assert!(seen.contains(&(ch::VIDEO, 0x8008, vec![0x08, 0x02])));
        assert!(seen.contains(&(ch::CLUSTER_VIDEO, 0x8008, vec![0x08, 0x02])));
        assert_eq!(seen.iter().filter(|s| s.0 == ch::INPUT).count(), 3);
        r.phone(0, 0x0010, &[]);
        assert_eq!(r.event().await, AaEvent::Disconnected { id: 7 });
        assert_eq!(r.events.recv().await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unanswered_goodbye_ends_after_a_second() {
        let mut r = rig(hevc(), false);
        r.running(None).await;
        r.cmds = mpsc::unbounded_channel().0;
        assert_eq!(r.event().await, AaEvent::Disconnected { id: 7 });
        loop {
            if r.frame().await.msg_id == 0x000f {
                break;
            }
        }
        assert_eq!(r.control().await, json!({ "type": "end" }));
        assert_eq!(r.control().await, json!({ "type": "close" }));
        assert_eq!(r.events.recv().await, None);
    }
}
