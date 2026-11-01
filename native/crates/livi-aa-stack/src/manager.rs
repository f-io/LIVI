// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::bridge::{Navigation, NowPlaying, PhoneStatus, Report};
use crate::channels::audio::AudioChannelType;
use crate::config::AaConfig;
use crate::discovery::VideoCodec;
use crate::driver::{self, SessionCmd};
use crate::helper_sock::{AaHelperSock, Feed};
use crate::link::{self, Link, LinkReader};
use crate::log::detail;
use crate::media::AaMedia;
use crate::protocol::PhoneCall;
use crate::sensors::Sensor;

const HELPER_RESUBSCRIBE: Duration = Duration::from_secs(2);

pub type SessionId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum AaEvent {
    HelperConnected,
    Helper(Value),
    Spawned {
        id: SessionId,
        wired: bool,
        usb_serial: Option<String>,
        peer: String,
    },
    Connected {
        id: SessionId,
    },
    Disconnected {
        id: SessionId,
    },
    Device {
        id: SessionId,
        name: String,
        model: String,
        instance_id: String,
        ip: String,
    },
    Status {
        id: SessionId,
        status: PhoneStatus,
    },
    Calls {
        id: SessionId,
        calls: Vec<PhoneCall>,
    },
    VideoCodec {
        id: SessionId,
        cluster: bool,
        codec: VideoCodec,
    },
    VideoFocus {
        id: SessionId,
        cluster: bool,
        projected: bool,
    },
    HostUiRequested {
        id: SessionId,
    },
    Audio {
        id: SessionId,
        stream: AudioChannelType,
        sample_rate: u32,
        channels: u32,
        active: bool,
    },
    Duck {
        id: SessionId,
        level: f64,
        duration_ms: u32,
    },
    Mic {
        id: SessionId,
        active: bool,
    },
    NowPlaying {
        id: SessionId,
        now_playing: NowPlaying,
    },
    Navigation {
        id: SessionId,
        navigation: Navigation,
    },
    NavigationImage {
        id: SessionId,
        image: Vec<u8>,
    },
}

impl AaEvent {
    pub(crate) fn from_report(id: SessionId, r: Report) -> Self {
        match r {
            Report::Connected => Self::Connected { id },
            Report::Disconnected => Self::Disconnected { id },
            Report::Device {
                name,
                model,
                instance_id,
                ip,
            } => Self::Device {
                id,
                name,
                model,
                instance_id,
                ip,
            },
            Report::Status(status) => Self::Status { id, status },
            Report::Calls(calls) => Self::Calls { id, calls },
            Report::VideoCodec { cluster, codec } => Self::VideoCodec { id, cluster, codec },
            Report::VideoFocus { cluster, projected } => Self::VideoFocus {
                id,
                cluster,
                projected,
            },
            Report::HostUiRequested => Self::HostUiRequested { id },
            Report::Audio {
                stream,
                sample_rate,
                channels,
                active,
            } => Self::Audio {
                id,
                stream,
                sample_rate,
                channels,
                active,
            },
            Report::Duck { level, duration_ms } => Self::Duck {
                id,
                level,
                duration_ms,
            },
            Report::NowPlaying(now_playing) => Self::NowPlaying { id, now_playing },
            Report::Navigation(navigation) => Self::Navigation { id, navigation },
            Report::NavigationImage(image) => Self::NavigationImage { id, image },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AaCmd {
    Session { id: SessionId, cmd: SessionCmd },
    Sensor(Sensor),
    ClusterActive(bool),
    StopWireless,
    Detach,
    Attach,
    Close { id: SessionId },
}

#[derive(Clone)]
pub struct AaHandle {
    cmds: mpsc::UnboundedSender<AaCmd>,
}

impl AaHandle {
    pub fn from_sender(cmds: mpsc::UnboundedSender<AaCmd>) -> Self {
        Self { cmds }
    }

    pub fn send(&self, cmd: AaCmd) {
        let _ = self.cmds.send(cmd);
    }

    pub fn session(&self, id: SessionId, cmd: SessionCmd) {
        self.send(AaCmd::Session { id, cmd });
    }
}

struct Entry {
    cmds: mpsc::UnboundedSender<SessionCmd>,
    wired: bool,
    wireless_peer: Option<String>,
}

struct Attached {
    link: Link,
    reader: LinkReader,
    wired: bool,
    usb_serial: Option<String>,
    peer: String,
}

struct Manager<M: AaMedia> {
    helper: AaHelperSock,
    media: Arc<M>,
    config: watch::Receiver<AaConfig>,
    events: mpsc::UnboundedSender<AaEvent>,
    session_events: mpsc::UnboundedSender<AaEvent>,
    attached_tx: mpsc::UnboundedSender<Attached>,
    next_id: SessionId,
    sessions: HashMap<SessionId, Entry>,
    attached: bool,
    cluster_stream_active: bool,
    night_mode: Option<bool>,
}

pub fn start<M: AaMedia>(
    helper: AaHelperSock,
    media: Arc<M>,
    config: watch::Receiver<AaConfig>,
    events: mpsc::UnboundedSender<AaEvent>,
) -> AaHandle {
    let (cmds, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(helper, media, config, cmd_rx, events));
    AaHandle { cmds }
}

async fn feed_recv(feed: &mut Option<mpsc::UnboundedReceiver<Feed>>) -> Option<Feed> {
    match feed {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn run<M: AaMedia>(
    helper: AaHelperSock,
    media: Arc<M>,
    mut config: watch::Receiver<AaConfig>,
    mut cmds: mpsc::UnboundedReceiver<AaCmd>,
    events: mpsc::UnboundedSender<AaEvent>,
) {
    let (session_events, mut session_rx) = mpsc::unbounded_channel();
    let (attached_tx, mut attached_rx) = mpsc::unbounded_channel();
    let night_mode = config.borrow_and_update().initial_night_mode;
    let mut m = Manager {
        helper,
        media,
        config: config.clone(),
        events,
        session_events,
        attached_tx,
        next_id: 1,
        sessions: HashMap::new(),
        attached: true,
        cluster_stream_active: true,
        night_mode,
    };
    let mut feed = Some(m.helper.subscribe(HELPER_RESUBSCRIBE));
    println!("[AaManager] Android Auto sessions come from the helper");
    let mut config_open = true;
    loop {
        tokio::select! {
            item = feed_recv(&mut feed) => match item {
                Some(Feed::Connected) => m.emit(AaEvent::HelperConnected),
                Some(Feed::Event(ev)) => m.on_helper_event(ev),
                None => feed = m.attached.then(|| m.helper.subscribe(HELPER_RESUBSCRIBE)),
            },
            Some(a) = attached_rx.recv() => m.on_attached(a),
            Some(ev) = session_rx.recv() => m.on_session_event(ev),
            changed = config.changed(), if config_open => match changed {
                Ok(()) => {
                    let night = config.borrow_and_update().initial_night_mode;
                    m.on_night_mode(night);
                }
                Err(_) => config_open = false,
            },
            cmd = cmds.recv() => match cmd {
                Some(AaCmd::Detach) => {
                    feed = None;
                    m.detach();
                }
                Some(AaCmd::Attach) => {
                    if !m.attached {
                        m.attached = true;
                        feed = Some(m.helper.subscribe(HELPER_RESUBSCRIBE));
                    }
                }
                Some(cmd) => m.on_cmd(cmd),
                None => break,
            },
        }
    }
    for entry in m.sessions.values() {
        let _ = entry.cmds.send(SessionCmd::Close);
    }
    let Manager {
        events,
        session_events,
        sessions,
        ..
    } = m;
    drop((session_events, sessions));
    while let Some(ev) = session_rx.recv().await {
        let _ = events.send(ev);
    }
}

impl<M: AaMedia> Manager<M> {
    fn emit(&self, event: AaEvent) {
        let _ = self.events.send(event);
    }

    fn on_helper_event(&mut self, ev: Value) {
        let socket = ev.get("socket").and_then(Value::as_str);
        let (Some("aa-session"), Some(socket)) = (ev.get("event").and_then(Value::as_str), socket)
        else {
            self.emit(AaEvent::Helper(ev));
            return;
        };
        let peer = ev
            .get("peer")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let wired = ev.get("transport").and_then(Value::as_str) == Some("usb");
        let usb_serial = ev
            .get("serial")
            .and_then(Value::as_str)
            .filter(|_| wired)
            .map(str::to_string);
        println!(
            "[AaManager] helper session {socket} from {peer}{}",
            if wired { " (usb)" } else { "" }
        );
        let socket = socket.to_string();
        let tx = self.attached_tx.clone();
        tokio::spawn(async move {
            match link::connect(&socket, &peer).await {
                Ok((link, reader)) => {
                    let _ = tx.send(Attached {
                        link,
                        reader,
                        wired,
                        usb_serial,
                        peer,
                    });
                }
                Err(e) => eprintln!("[AaManager] helper session {socket}: {e}"),
            }
        });
    }

    fn on_attached(&mut self, a: Attached) {
        let Attached {
            mut link,
            reader,
            wired,
            usb_serial,
            peer,
        } = a;
        if !self.attached {
            link.destroy();
            return;
        }
        if !wired {
            self.supersede_wireless(&peer);
        }
        let id = self.next_id;
        self.next_id += 1;
        let (cmds, cmd_rx) = mpsc::unbounded_channel();
        self.sessions.insert(
            id,
            Entry {
                cmds,
                wired,
                wireless_peer: (!wired).then(|| peer.clone()),
            },
        );
        self.emit(AaEvent::Spawned {
            id,
            wired,
            usb_serial,
            peer,
        });
        tokio::spawn(driver::run(
            id,
            link,
            reader,
            self.config.clone(),
            self.media.clone(),
            self.cluster_stream_active,
            cmd_rx,
            self.session_events.clone(),
        ));
    }

    fn supersede_wireless(&mut self, ip: &str) {
        if ip.is_empty() {
            return;
        }
        for entry in self.sessions.values() {
            if entry.wireless_peer.as_deref() == Some(ip) {
                println!(
                    "[AaManager] wireless reconnect from {ip}, dropping the superseded session"
                );
                let _ = entry.cmds.send(SessionCmd::Close);
            }
        }
    }

    fn on_session_event(&mut self, ev: AaEvent) {
        if let AaEvent::Disconnected { id } = ev {
            self.sessions.remove(&id);
        }
        self.emit(ev);
    }

    fn each(&self, cmd: SessionCmd) {
        for entry in self.sessions.values() {
            let _ = entry.cmds.send(cmd.clone());
        }
    }

    fn on_night_mode(&mut self, night: Option<bool>) {
        if night == self.night_mode {
            return;
        }
        self.night_mode = night;
        detail!("[AaManager] nightMode={night:?}");
        if let Some(night) = night {
            self.each(SessionCmd::Sensor(Sensor::NightMode(night)));
        }
    }

    fn detach(&mut self) {
        self.attached = false;
        for entry in self.sessions.values() {
            let _ = entry.cmds.send(SessionCmd::Close);
        }
    }

    fn on_cmd(&mut self, cmd: AaCmd) {
        match cmd {
            AaCmd::Session { id, cmd } => {
                if let Some(entry) = self.sessions.get(&id) {
                    let _ = entry.cmds.send(cmd);
                }
            }
            AaCmd::Sensor(s) => self.each(SessionCmd::Sensor(s)),
            AaCmd::ClusterActive(active) => {
                self.cluster_stream_active = active;
                self.each(SessionCmd::ClusterActive(active));
            }
            AaCmd::StopWireless => {
                for entry in self.sessions.values().filter(|e| !e.wired) {
                    let _ = entry.cmds.send(SessionCmd::Close);
                }
            }
            AaCmd::Close { id } => {
                if let Some(entry) = self.sessions.get(&id) {
                    let _ = entry.cmds.send(SessionCmd::Close);
                }
            }
            AaCmd::Detach | AaCmd::Attach => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::net::unix::OwnedWriteHalf;

    use super::*;
    use crate::channels::Frame;
    use crate::commands::Command;
    use crate::helper_sock::tests::Dir;
    use crate::link::{LinkItem, attach};
    use crate::media::tests::FakeMedia;

    type Subscriber = Arc<tokio::sync::Mutex<Option<OwnedWriteHalf>>>;

    fn fake_helper(path: &Path) -> Subscriber {
        let listener = UnixListener::bind(path).unwrap();
        let sub: Subscriber = Arc::new(tokio::sync::Mutex::new(None));
        let slot = sub.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (rd, mut wr) = sock.into_split();
                let mut line = String::new();
                if BufReader::new(rd).read_line(&mut line).await.is_err() {
                    continue;
                }
                if line.trim() == "subscribe" {
                    *slot.lock().await = Some(wr);
                } else {
                    let _ = wr.write_all(b"{\"ok\":true}\n").await;
                }
            }
        });
        sub
    }

    struct Phone {
        link: Link,
        from: LinkReader,
    }

    impl Phone {
        async fn frame(&mut self) -> Frame {
            loop {
                match tokio::time::timeout(Duration::from_secs(5), self.from.next())
                    .await
                    .unwrap()
                {
                    LinkItem::Message(f) if f.ch == 0 && f.msg_id == 0x000b => {}
                    LinkItem::Message(f) => return f,
                    LinkItem::Control(_) => {}
                    LinkItem::Closed(e) => panic!("closed {e:?}"),
                }
            }
        }

        async fn until(&mut self, ch: u8, msg_id: u16) -> Frame {
            loop {
                let f = self.frame().await;
                if (f.ch, f.msg_id) == (ch, msg_id) {
                    return f;
                }
            }
        }
    }

    struct Rig {
        dir: Dir,
        sub: Subscriber,
        handle: AaHandle,
        events: mpsc::UnboundedReceiver<AaEvent>,
        cfg: watch::Sender<AaConfig>,
        sockets: u32,
    }

    impl Rig {
        async fn new() -> Self {
            let dir = Dir::new();
            let path = dir.0.join("aa.sock");
            let sub = fake_helper(&path);
            let (cfg, cfg_rx) = watch::channel(AaConfig::default());
            let (ev_tx, events) = mpsc::unbounded_channel();
            let handle = start(AaHelperSock::new(&path), FakeMedia::new(), cfg_rx, ev_tx);
            let mut rig = Self {
                dir,
                sub,
                handle,
                events,
                cfg,
                sockets: 0,
            };
            assert_eq!(rig.event().await, AaEvent::HelperConnected);
            rig
        }

        async fn event(&mut self) -> AaEvent {
            tokio::time::timeout(Duration::from_secs(5), self.events.recv())
                .await
                .unwrap()
                .unwrap()
        }

        async fn until(&mut self, want: impl Fn(&AaEvent) -> bool) -> AaEvent {
            loop {
                let ev = self.event().await;
                if want(&ev) {
                    return ev;
                }
            }
        }

        async fn push(&self, event: Value) {
            for _ in 0..500 {
                let mut slot = self.sub.lock().await;
                if let Some(wr) = slot.as_mut() {
                    if wr.write_all(format!("{event}\n").as_bytes()).await.is_ok() {
                        return;
                    }
                    *slot = None;
                }
                drop(slot);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("nobody subscribed");
        }

        fn socket(&mut self) -> PathBuf {
            self.sockets += 1;
            self.dir.0.join(format!("aa-session-{}.sock", self.sockets))
        }

        async fn phone(
            &mut self,
            peer: &str,
            transport: &str,
            serial: Option<&str>,
        ) -> (SessionId, Phone) {
            let path = self.socket();
            let listener = UnixListener::bind(&path).unwrap();
            let mut ev = json!({ "event": "aa-session", "socket": path, "peer": peer, "transport": transport });
            if let Some(serial) = serial {
                ev["serial"] = json!(serial);
            }
            self.push(ev).await;
            let (sock, _) = listener.accept().await.unwrap();
            let (link, from) = attach(sock, "");
            let AaEvent::Spawned {
                id,
                wired,
                usb_serial,
                peer: got,
            } = self.until(|e| matches!(e, AaEvent::Spawned { .. })).await
            else {
                unreachable!()
            };
            assert_eq!(wired, transport == "usb");
            assert_eq!(usb_serial.as_deref(), if wired { serial } else { None });
            assert_eq!(got, peer);
            (id, Phone { link, from })
        }

        async fn running(&mut self, id: SessionId, phone: &mut Phone) {
            phone.link.control(&json!({ "type": "ready" }));
            phone.link.send(&Frame::new(0, 0x0b, 0x0005, []));
            phone.link.send(&Frame::new(3, 0x0b, 0x8000, [0x08, 0x03]));
            self.until(|e| *e == AaEvent::Connected { id }).await;
            phone.until(3, 0x8008).await;
        }
    }

    #[tokio::test]
    async fn sessions_come_from_the_helper_and_hear_core() {
        let mut rig = Rig::new().await;
        rig.push(json!({ "event": "hfp", "up": true, "mac": "aa" }))
            .await;
        assert_eq!(
            rig.event().await,
            AaEvent::Helper(json!({ "event": "hfp", "up": true, "mac": "aa" }))
        );
        rig.push(json!({ "event": "aa-session" })).await;
        rig.push(json!({ "type": "not an event" })).await;
        let (a, mut pa) = rig.phone("10.0.0.2", "wifi", Some("ignored")).await;
        let (b, mut pb) = rig.phone("usb-1", "usb", Some("S1")).await;
        rig.running(a, &mut pa).await;

        rig.handle.send(AaCmd::Sensor(Sensor::Gear(5)));
        assert_eq!(pa.until(1, 0x8003).await.payload, [0x42, 0x02, 0x08, 0x05]);
        rig.handle.send(AaCmd::ClusterActive(false));
        assert_eq!(pa.until(19, 0x8008).await.payload, [0x08, 0x02]);
        rig.cfg.send_modify(|c| c.initial_night_mode = Some(true));
        assert_eq!(pa.until(1, 0x8003).await.payload, [0x52, 0x02, 0x08, 0x01]);
        rig.cfg.send_modify(|c| c.hevc_supported = true);
        rig.handle.session(a, SessionCmd::Command(Command::Home));
        assert_eq!(pa.until(8, 0x8001).await.ch, 8);
        rig.handle.session(99, SessionCmd::Close);
        rig.handle.send(AaCmd::Close { id: 99 });

        rig.handle.send(AaCmd::StopWireless);
        assert_eq!(
            rig.until(|e| matches!(e, AaEvent::Disconnected { .. }))
                .await,
            AaEvent::Disconnected { id: a }
        );
        pa.until(0, 0x000f).await;
        rig.handle.send(AaCmd::Close { id: b });
        assert_eq!(
            rig.until(|e| matches!(e, AaEvent::Disconnected { .. }))
                .await,
            AaEvent::Disconnected { id: b }
        );
        drop(pb.link);
        let _ = pb.from.next().await;
    }

    #[tokio::test]
    async fn a_wireless_reconnect_replaces_the_old_session() {
        let mut rig = Rig::new().await;
        let (old, _p_old) = rig.phone("10.0.0.3", "wifi", None).await;
        let (wired, _p_wired) = rig.phone("10.0.0.3", "usb", None).await;
        let path = rig.socket();
        let listener = UnixListener::bind(&path).unwrap();
        rig.push(json!({ "event": "aa-session", "socket": path, "peer": "10.0.0.3", "transport": "wifi" })).await;
        let _kept = listener.accept().await.unwrap();
        let mut seen = [rig.event().await, rig.event().await];
        seen.sort_by_key(|e| matches!(e, AaEvent::Spawned { .. }));
        assert_eq!(seen[0], AaEvent::Disconnected { id: old });
        assert!(matches!(seen[1], AaEvent::Spawned { wired: false, .. }));
        assert_ne!(old, wired);
    }

    #[tokio::test]
    async fn detach_ends_every_session_and_attach_listens_again() {
        let mut rig = Rig::new().await;
        let (a, _pa) = rig.phone("10.0.0.4", "wifi", None).await;
        rig.push(
            json!({ "event": "aa-session", "socket": rig.dir.0.join("nothing.sock"), "peer": "x" }),
        )
        .await;
        rig.push(json!({ "event": "sco", "up": false })).await;
        assert_eq!(
            rig.event().await,
            AaEvent::Helper(json!({ "event": "sco", "up": false }))
        );
        rig.handle.send(AaCmd::Detach);
        assert_eq!(rig.event().await, AaEvent::Disconnected { id: a });
        rig.handle.send(AaCmd::Attach);
        rig.handle.send(AaCmd::Attach);
        assert_eq!(rig.event().await, AaEvent::HelperConnected);
        let (b, _pb) = rig.phone("10.0.0.5", "wifi", None).await;
        rig.handle = AaHandle::from_sender(mpsc::unbounded_channel().0);
        assert_eq!(rig.event().await, AaEvent::Disconnected { id: b });
    }

    #[test]
    fn reports_carry_the_session_id() {
        assert_eq!(
            AaEvent::from_report(3, Report::HostUiRequested),
            AaEvent::HostUiRequested { id: 3 }
        );
        assert_eq!(
            AaEvent::from_report(3, Report::NavigationImage(vec![1])),
            AaEvent::NavigationImage {
                id: 3,
                image: vec![1]
            }
        );
        assert_eq!(
            AaEvent::from_report(3, Report::Calls(vec![])),
            AaEvent::Calls {
                id: 3,
                calls: vec![]
            }
        );
    }
}
