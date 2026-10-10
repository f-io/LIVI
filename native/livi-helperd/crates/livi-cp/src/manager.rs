//! One connection is one session, and a session born at iAP2 identification
//! gives the phone's metadata a target until it connects.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::helper_sock::Feed;
use crate::media::Media;
use crate::net::{host_of, norm_host};
use crate::stack::{self, AudioProfile, ScreenCodec, StackCmd, StackCtx, StackEvent};

const RESTART_FIRST: Duration = Duration::from_millis(500);
const RESTART_EVERY: Duration = Duration::from_millis(1000);
const RESTART_TRIES: u32 = 10;
const KEEPALIVE_IDLE: Duration = Duration::from_secs(3);
const PENDING_MAX: usize = 8;

pub type SessionId = u64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceIds {
    pub bt_mac: Option<String>,
    pub wifi_mac: Option<String>,
    pub ip: Option<String>,
    pub usb_udid: Option<String>,
    pub controller_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Presence {
    /// RECORD: the session runs.
    Active { ip: String },
    Device {
        bt_mac: String,
        wifi_mac: String,
        ip: String,
        usb_udid: String,
        name: String,
        model: String,
    },
    Status {
        battery_level: Option<f64>,
        battery_charging: Option<bool>,
        signal_strength: Option<f64>,
        carrier_name: Option<String>,
    },
}

/// Independent of any session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperPresence {
    Wifi { wifi_mac: String, ip: String, connected: bool },
    Device { ids: DeviceIds },
    DeviceGone { usb_udid: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    Ringing,
    Active,
    Ended,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CpEvent {
    Spawned {
        id: SessionId,
    },
    /// Sent once the main screen stream is set up.
    Connected {
        id: SessionId,
        wired: bool,
        controller_id: Option<String>,
    },
    Disconnected {
        id: SessionId,
    },
    Presence {
        id: SessionId,
        wired: bool,
        presence: Presence,
    },
    HelperPresence(HelperPresence),
    HelperConnected,
    /// Summer time included.
    PhoneUtcOffset {
        minutes: i32,
    },
    VideoCodec {
        id: SessionId,
        cluster: bool,
        codec: ScreenCodec,
    },
    AudioActive {
        id: SessionId,
        profile: AudioProfile,
        active: bool,
    },
    Duck {
        id: SessionId,
        level: f64,
        duration_ms: u32,
    },
    HostUiRequested {
        id: SessionId,
    },
    SpeechActive {
        id: SessionId,
        active: bool,
    },
    Call {
        id: SessionId,
        phase: CallPhase,
    },
    Metadata {
        id: SessionId,
        event: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CpCmd {
    Stack {
        id: SessionId,
        cmd: StackCmd,
    },
    NightMode(bool),
    /// None for automatic.
    InitialNightMode(Option<bool>),
    LimitedUi(bool),
    ClusterActive(bool),
    /// The phones reconnect.
    DropSessions,
    Close {
        id: SessionId,
    },
}

#[derive(Clone)]
pub struct CpHandle {
    cmds: mpsc::UnboundedSender<CpCmd>,
}

impl CpHandle {
    pub fn from_sender(cmds: mpsc::UnboundedSender<CpCmd>) -> Self {
        Self { cmds }
    }

    pub fn send(&self, cmd: CpCmd) {
        let _ = self.cmds.send(cmd);
    }

    pub fn stack(&self, id: SessionId, cmd: StackCmd) {
        self.send(CpCmd::Stack { id, cmd });
    }
}

#[derive(Default)]
struct Session {
    /// None for a session born at identification.
    stack: Option<mpsc::UnboundedSender<StackCmd>>,
    bt_mac: String,
    wifi_mac: String,
    peer_ip: String,
    local_ip: String,
    usb_udid: String,
    /// Set by pair-verify.
    controller_id: Option<String>,
    connected: bool,
}

impl Session {
    fn send(&self, cmd: StackCmd) {
        if let Some(stack) = &self.stack {
            let _ = stack.send(cmd);
        }
    }

    fn matches(&self, ids: &DeviceIds) -> bool {
        let same = |a: &Option<String>, b: &str| {
            a.as_deref()
                .is_some_and(|a| !a.is_empty() && !b.is_empty() && a.eq_ignore_ascii_case(b))
        };
        same(&ids.bt_mac, &self.bt_mac)
            || same(&ids.wifi_mac, &self.wifi_mac)
            || ids.ip.as_deref().is_some_and(|ip| {
                !ip.is_empty() && !self.peer_ip.is_empty() && norm_host(ip) == self.peer_ip
            })
            || ids.usb_udid.as_deref().is_some_and(|u| !u.is_empty() && u == self.usb_udid)
            || matches!((&ids.controller_id, &self.controller_id), (Some(a), Some(b)) if !a.is_empty() && a == b)
    }
}

fn str_of(ev: &Value, key: &str) -> String {
    ev.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn opt(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

struct Manager<M: Media> {
    ctx: Arc<StackCtx<M>>,
    events: mpsc::UnboundedSender<CpEvent>,
    stack_events: mpsc::UnboundedSender<(SessionId, StackEvent)>,
    ticks: mpsc::UnboundedSender<(String, u32)>,
    next_id: SessionId,
    sessions: HashMap<SessionId, Session>,
    /// The session that owns the helper's single metadata feed.
    live: Option<SessionId>,
    /// Waiting for their session.
    pending: Vec<DeviceIds>,
    /// btMac (lower case) to usbUdid of the phones on the bus.
    wired: HashMap<String, String>,
    /// Phones whose wireless session ended, their last events must not make a new one.
    gone: HashSet<String>,
    /// Wireless sessions whose phone was plugged in while they ran.
    cabled: HashSet<SessionId>,
    /// usbUdid to the address of ours the phone was told to reach over its cable.
    cable_addrs: HashMap<String, String>,
    /// usbUdid to the next start, for a phone yet to connect over its cable.
    to_cable: HashMap<String, JoinHandle<()>>,
    initial_night_mode: Option<bool>,
    cluster_stream_active: bool,
}

pub fn start<M: Media>(
    listener: TcpListener,
    ctx: Arc<StackCtx<M>>,
    events: mpsc::UnboundedSender<CpEvent>,
) -> CpHandle {
    let (cmds, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(listener, ctx, cmd_rx, events));
    CpHandle { cmds }
}

async fn run<M: Media>(
    listener: TcpListener,
    ctx: Arc<StackCtx<M>>,
    mut cmds: mpsc::UnboundedReceiver<CpCmd>,
    events: mpsc::UnboundedSender<CpEvent>,
) {
    let mut feed = ctx.helper.subscribe();
    let (stack_events, mut stack_rx) = mpsc::unbounded_channel();
    let (ticks, mut tick_rx) = mpsc::unbounded_channel();
    let mut m = Manager {
        ctx,
        events,
        stack_events,
        ticks,
        next_id: 1,
        sessions: HashMap::new(),
        live: None,
        pending: Vec::new(),
        wired: HashMap::new(),
        gone: HashSet::new(),
        cabled: HashSet::new(),
        cable_addrs: HashMap::new(),
        to_cable: HashMap::new(),
        initial_night_mode: None,
        cluster_stream_active: true,
    };
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((sock, _)) = accepted {
                    m.spawn(sock);
                }
            }
            item = feed.recv() => match item {
                Some(Feed::Connected) => m.emit(CpEvent::HelperConnected),
                Some(Feed::Event(ev)) => m.on_helper_event(&ev),
                None => feed = m.ctx.helper.subscribe(),
            },
            Some((id, ev)) = stack_rx.recv() => m.on_stack_event(id, ev),
            Some((udid, attempt)) = tick_rx.recv() => m.start_tick(&udid, attempt),
            cmd = cmds.recv() => match cmd {
                Some(cmd) => m.on_cmd(cmd),
                None => break,
            },
        }
    }
    for id in m.sessions.keys().copied().collect::<Vec<_>>() {
        m.close(id);
    }
    for (_, timer) in m.to_cable.drain() {
        timer.abort();
    }
}

impl<M: Media> Manager<M> {
    fn emit(&self, event: CpEvent) {
        let _ = self.events.send(event);
    }

    fn presence(&self, id: SessionId, presence: Presence) {
        self.emit(CpEvent::Presence { id, wired: self.is_wired(id), presence });
    }

    fn spawn(&mut self, sock: TcpStream) {
        let peer = sock.peer_addr().map(|a| host_of(&a)).unwrap_or_default();
        let local = sock.local_addr().map(|a| host_of(&a)).unwrap_or_default();
        println!("[CpManager] control connection from {peer}");
        let _ =
            SockRef::from(&sock).set_tcp_keepalive(&TcpKeepalive::new().with_time(KEEPALIVE_IDLE));
        if let Some(udid) = self.cable_udid(&local) {
            self.came_over(&udid);
        }
        let (stack, stack_rx) = mpsc::unbounded_channel();
        let id = self.register(Session {
            stack: Some(stack),
            peer_ip: norm_host(&peer),
            local_ip: norm_host(&local),
            ..Default::default()
        });
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        let forward = self.stack_events.clone();
        tokio::spawn(async move {
            while let Some(ev) = ev_rx.recv().await {
                if forward.send((id, ev)).is_err() {
                    return;
                }
            }
        });
        tokio::spawn(stack::run(sock, self.ctx.clone(), stack_rx, ev_tx));
    }

    fn register(&mut self, session: Session) -> SessionId {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(night) = self.initial_night_mode {
            session.send(StackCmd::NightMode(night));
        }
        session.send(StackCmd::ClusterActive(self.cluster_stream_active));
        self.sessions.insert(id, session);
        self.emit(CpEvent::Spawned { id });
        id
    }

    fn cable_udid(&self, addr: &str) -> Option<String> {
        let ip = norm_host(addr);
        self.cable_addrs.iter().find(|(_, a)| **a == ip).map(|(udid, _)| udid.clone())
    }

    /// A connected session is wired when it reached the address given over the
    /// cable. Before that, it is wired when its iAP2 came over the cable.
    fn is_wired(&self, id: SessionId) -> bool {
        let Some(s) = self.sessions.get(&id) else { return false };
        if s.local_ip.is_empty() {
            !s.usb_udid.is_empty()
        } else {
            self.cable_udid(&s.local_ip).is_some()
        }
    }

    fn on_cable(&self, id: SessionId) -> bool {
        self.sessions.get(&id).is_some_and(|s| {
            self.wired.contains_key(&s.bt_mac.to_lowercase()) && !self.cabled.contains(&id)
        })
    }

    fn on_stack_event(&mut self, id: SessionId, ev: StackEvent) {
        if !self.sessions.contains_key(&id) {
            return;
        }
        match ev {
            StackEvent::DeviceInfo { name, device_id, wifi_mac, model } => {
                let s = self.sessions.get_mut(&id).expect("checked above");
                if s.bt_mac.is_empty() && !device_id.is_empty() {
                    s.bt_mac = device_id;
                    s.send(StackCmd::PhoneBtMac(s.bt_mac.clone()));
                }
                s.wifi_mac = wifi_mac;
                let (bt_mac, wifi_mac) = (s.bt_mac.clone(), s.wifi_mac.clone());
                self.adopt_pending(id);
                self.presence(
                    id,
                    Presence::Device {
                        bt_mac,
                        wifi_mac,
                        ip: String::new(),
                        usb_udid: String::new(),
                        name,
                        model,
                    },
                );
            }
            StackEvent::Active { ip, controller_id } => {
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.controller_id = controller_id;
                }
                // A session reaching RECORD supersedes an earlier connection of
                // the same phone, the Bluetooth to Wi-Fi handover.
                self.supersede(id);
                self.presence(id, Presence::Active { ip });
            }
            StackEvent::Closed { .. } => self.close(id),
            StackEvent::VideoCodec { cluster, codec } => {
                self.emit(CpEvent::VideoCodec { id, cluster, codec });
            }
            StackEvent::MainScreenReady => {
                let s = self.sessions.get_mut(&id).expect("checked above");
                if !s.connected {
                    s.connected = true;
                    let controller_id = s.controller_id.clone();
                    self.live = Some(id);
                    self.emit(CpEvent::Connected { id, wired: self.is_wired(id), controller_id });
                }
            }
            StackEvent::AudioActive { profile, active } => {
                self.emit(CpEvent::AudioActive { id, profile, active });
            }
            StackEvent::MicActive { .. } => {}
            StackEvent::Duck { level, duration_ms } => {
                self.emit(CpEvent::Duck { id, level, duration_ms });
            }
            StackEvent::HostUiRequested => self.emit(CpEvent::HostUiRequested { id }),
            StackEvent::SpeechActive(active) => self.emit(CpEvent::SpeechActive { id, active }),
            StackEvent::DisableBluetooth(device) => {
                let mac = device.trim().to_string();
                if mac.is_empty() {
                    return;
                }
                let helper = self.ctx.helper.clone();
                tokio::spawn(async move {
                    match helper.disconnect_bt(&mac).await {
                        Ok(()) => {
                            println!("[CpSession] BT disconnected for {mac} — iAP2 moves to tunnel")
                        }
                        Err(e) => println!("[CpSession] disconnectBt failed: {e}"),
                    }
                });
            }
        }
    }

    fn close(&mut self, id: SessionId) {
        let Some(s) = self.sessions.remove(&id) else { return };
        s.send(StackCmd::Stop);
        let mac = s.bt_mac.to_lowercase();
        match self.wired.get(&mac).cloned() {
            Some(udid) => {
                if self.cabled.remove(&id) {
                    self.start_again(&udid, 1);
                }
            }
            None if !mac.is_empty() => {
                self.gone.insert(mac);
            }
            None => {}
        }
        self.cabled.remove(&id);
        if self.live == Some(id) {
            self.live = self.sessions.keys().max().copied();
        }
        self.emit(CpEvent::Disconnected { id });
    }

    /// Right after its wireless session the phone drops a start without a
    /// word, so it hears it again until it connects over the cable.
    fn start_again(&mut self, udid: &str, attempt: u32) {
        if let Some(old) = self.to_cable.remove(udid) {
            old.abort();
        }
        let ticks = self.ticks.clone();
        let key = udid.to_string();
        let delay = if attempt == 1 { RESTART_FIRST } else { RESTART_EVERY };
        let timer = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = ticks.send((key, attempt));
        });
        self.to_cable.insert(udid.to_string(), timer);
    }

    fn start_tick(&mut self, udid: &str, attempt: u32) {
        if self.to_cable.remove(udid).is_none() {
            return;
        }
        println!("[CpManager] {udid} hears the start again ({attempt}/{RESTART_TRIES})");
        let helper = self.ctx.helper.clone();
        let target = udid.to_string();
        tokio::spawn(async move {
            let _ = helper.start_wired(&target).await;
        });
        if attempt < RESTART_TRIES {
            self.start_again(udid, attempt + 1);
        }
    }

    fn came_over(&mut self, udid: &str) {
        if let Some(timer) = self.to_cable.remove(udid) {
            timer.abort();
        }
    }

    fn supersede(&mut self, keep: SessionId) {
        let Some(cid) = self.sessions.get(&keep).and_then(|s| s.controller_id.clone()) else {
            return;
        };
        let others: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(id, s)| **id != keep && s.controller_id.as_deref() == Some(cid.as_str()))
            .map(|(id, _)| *id)
            .collect();
        for other in others {
            println!("[CpManager] transport handover: dropping the superseded connection");
            self.cabled.remove(&other);
            self.close(other);
        }
    }

    fn adopt_helper_device(&mut self, id: SessionId, ids: &DeviceIds) {
        let Some(s) = self.sessions.get_mut(&id) else { return };
        if let Some(mac) = ids.bt_mac.as_ref().filter(|m| !m.is_empty()) {
            s.bt_mac = mac.clone();
            s.send(StackCmd::PhoneBtMac(mac.clone()));
        }
        if let Some(udid) = ids.usb_udid.as_ref().filter(|u| !u.is_empty()) {
            s.usb_udid = udid.clone();
        }
        let presence = Presence::Device {
            bt_mac: ids
                .bt_mac
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| s.bt_mac.clone()),
            wifi_mac: s.wifi_mac.clone(),
            ip: ids.ip.clone().unwrap_or_default(),
            usb_udid: ids.usb_udid.clone().unwrap_or_default(),
            name: ids.name.clone().unwrap_or_default(),
            model: String::new(),
        };
        self.presence(id, presence);
    }

    fn adopt_pending(&mut self, id: SessionId) {
        let mut i = self.pending.len();
        while i > 0 {
            i -= 1;
            let matches = self.sessions.get(&id).is_some_and(|s| s.matches(&self.pending[i]));
            if matches {
                let ids = self.pending.remove(i);
                self.adopt_helper_device(id, &ids);
            }
        }
    }

    fn buffer_pending(&mut self, ids: DeviceIds) {
        if ids.bt_mac.is_none() && ids.wifi_mac.is_none() && ids.usb_udid.is_none() {
            return;
        }
        let same = self.pending.iter().position(|d| {
            matches!((&ids.bt_mac, &d.bt_mac), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b))
                || matches!((&ids.usb_udid, &d.usb_udid), (Some(a), Some(b)) if a == b)
        });
        match same {
            Some(i) => {
                let d = &mut self.pending[i];
                let merge = |into: &mut Option<String>, from: &Option<String>| {
                    if from.is_some() {
                        into.clone_from(from);
                    }
                };
                merge(&mut d.bt_mac, &ids.bt_mac);
                merge(&mut d.wifi_mac, &ids.wifi_mac);
                merge(&mut d.ip, &ids.ip);
                merge(&mut d.usb_udid, &ids.usb_udid);
                merge(&mut d.name, &ids.name);
            }
            None => self.pending.push(ids),
        }
        while self.pending.len() > PENDING_MAX {
            self.pending.remove(0);
        }
    }

    fn create_meta_session(&mut self, phone_id: &str) -> SessionId {
        let id = self.register(Session::default());
        let ids = DeviceIds {
            bt_mac: Some(phone_id.to_string()),
            usb_udid: self.wired.get(&phone_id.to_lowercase()).cloned(),
            ..Default::default()
        };
        self.adopt_helper_device(id, &ids);
        self.adopt_pending(id);
        id
    }

    fn match_session(&self, ids: &DeviceIds) -> Option<SessionId> {
        let mut ids_sorted: Vec<_> = self.sessions.iter().collect();
        ids_sorted.sort_by_key(|(id, _)| **id);
        ids_sorted.into_iter().find(|(_, s)| s.matches(ids)).map(|(id, _)| *id)
    }

    fn metadata_target(&self) -> Option<SessionId> {
        match self.live {
            Some(id) if self.sessions.contains_key(&id) => Some(id),
            _ if self.sessions.len() == 1 => self.sessions.keys().next().copied(),
            _ => None,
        }
    }

    fn on_helper_event(&mut self, ev: &Value) {
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "wifi" => {
                let wifi_mac = str_of(ev, "mac");
                let connected = str_of(ev, "event") == "joined";
                if !connected && !wifi_mac.is_empty() {
                    self.left_wifi(&wifi_mac);
                }
                self.emit(CpEvent::HelperPresence(HelperPresence::Wifi {
                    wifi_mac,
                    ip: str_of(ev, "ip"),
                    connected,
                }));
            }
            "device" => {
                let ids = DeviceIds {
                    bt_mac: opt(str_of(ev, "btMac")),
                    ip: opt(str_of(ev, "ip")),
                    usb_udid: opt(str_of(ev, "usbUdid")),
                    name: opt(str_of(ev, "name")),
                    ..Default::default()
                };
                if let Some(mac) = &ids.bt_mac {
                    self.gone.remove(&mac.to_lowercase());
                }
                let found = self.match_session(&ids);
                if let (Some(mac), Some(udid)) = (&ids.bt_mac, &ids.usb_udid) {
                    let mac = mac.to_lowercase();
                    // A session already running when the phone comes onto the bus is
                    // the wireless one.
                    if !self.wired.contains_key(&mac)
                        && let Some(id) = found
                        && self.sessions[&id].controller_id.is_some()
                    {
                        self.cabled.insert(id);
                    }
                    self.wired.insert(mac, udid.clone());
                }
                self.emit(CpEvent::HelperPresence(HelperPresence::Device { ids: ids.clone() }));
                match found {
                    Some(id) => self.adopt_helper_device(id, &ids),
                    None => self.buffer_pending(ids),
                }
            }
            "link" => {
                // The LIVI Link carries every wireless session, they end with it.
                if ev.get("up") == Some(&Value::Bool(false)) {
                    let wireless: Vec<SessionId> =
                        self.sessions.keys().copied().filter(|id| !self.on_cable(*id)).collect();
                    for id in wireless {
                        self.close(id);
                    }
                    self.pending.retain(|d| d.usb_udid.is_some());
                }
            }
            "wired-start" => {
                let udid = str_of(ev, "usbUdid");
                let ip = norm_host(&str_of(ev, "ip"));
                if udid.is_empty() || ip.is_empty() {
                    return;
                }
                self.cable_addrs.insert(udid.clone(), ip);
                // The phone runs one session at a time, so its wireless one makes room.
                let ids = DeviceIds {
                    usb_udid: Some(udid.clone()),
                    bt_mac: opt(str_of(ev, "phoneId")),
                    ..Default::default()
                };
                let leaving: Vec<SessionId> = self
                    .cabled
                    .iter()
                    .copied()
                    .filter(|id| self.sessions.get(id).is_some_and(|s| s.matches(&ids)))
                    .collect();
                for id in leaving {
                    println!(
                        "[CpManager] {udid} got the start over its cable, wireless makes room"
                    );
                    self.close(id);
                }
            }
            "deviceTime" => {
                if let Some(minutes) = ev.get("utcOffsetMinutes").and_then(Value::as_f64) {
                    self.emit(CpEvent::PhoneUtcOffset { minutes: minutes as i32 });
                }
            }
            "device-gone" => {
                let udid = str_of(ev, "usbUdid");
                if udid.is_empty() {
                    return;
                }
                self.wired.retain(|_, u| *u != udid);
                self.cable_addrs.remove(&udid);
                self.came_over(&udid);
                self.emit(CpEvent::HelperPresence(HelperPresence::DeviceGone {
                    usb_udid: udid.clone(),
                }));
                let ids = DeviceIds { usb_udid: Some(udid.clone()), ..Default::default() };
                let matching: Vec<SessionId> = self
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.matches(&ids))
                    .map(|(id, _)| *id)
                    .collect();
                for id in matching {
                    // A session that ran before the cable came is the wireless one, it
                    // carries on.
                    if self.cabled.remove(&id) {
                        continue;
                    }
                    self.close(id);
                }
                self.pending.retain(|d| d.usb_udid.as_deref() != Some(udid.as_str()));
            }
            _ => self.route_metadata(ev),
        }
    }

    /// A phone that left the access point ended CarPlay over Wi-Fi itself, one
    /// running over the cable stays.
    fn left_wifi(&mut self, wifi_mac: &str) {
        let ids = DeviceIds { wifi_mac: Some(wifi_mac.to_string()), ..Default::default() };
        let leaving: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.matches(&ids))
            .map(|(id, _)| *id)
            .filter(|id| !self.on_cable(*id))
            .collect();
        for id in leaving {
            println!("[CpManager] {wifi_mac} left the access point, its session ends");
            self.close(id);
        }
    }

    /// Metadata carries the phone's iAP2 identity, the same over Bluetooth and
    /// both tunnels, and its controller id.
    fn route_metadata(&mut self, ev: &Value) {
        let phone_id = str_of(ev, "phoneId");
        let cid = str_of(ev, "cid");
        let by = |ids: DeviceIds| self.match_session(&ids);
        let mut target = None;
        if !phone_id.is_empty() {
            target = by(DeviceIds { bt_mac: Some(phone_id.clone()), ..Default::default() });
        }
        if target.is_none() && !cid.is_empty() {
            target = by(DeviceIds { controller_id: Some(cid.clone()), ..Default::default() });
        }
        if target.is_none() {
            let fallback = self.metadata_target();
            let contradicts = !phone_id.is_empty()
                && fallback.is_some_and(|id| {
                    let mac = &self.sessions[&id].bt_mac;
                    !mac.is_empty() && !mac.eq_ignore_ascii_case(&phone_id)
                });
            let unattributable = phone_id.is_empty() && cid.is_empty() && self.sessions.len() > 1;
            if unattributable {
                target = None;
            } else if fallback.is_some() && !contradicts {
                target = fallback;
            } else if !phone_id.is_empty() && !self.gone.contains(&phone_id.to_lowercase()) {
                target = Some(self.create_meta_session(&phone_id));
            }
        }
        if let Some(id) = target {
            self.ingest(id, ev);
        }
    }

    fn ingest(&mut self, id: SessionId, ev: &Value) {
        let num = |k: &str| ev.get(k).and_then(Value::as_f64);
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "power" => self.presence(
                id,
                Presence::Status {
                    battery_level: num("level"),
                    battery_charging: ev.get("charging").and_then(Value::as_bool),
                    signal_strength: None,
                    carrier_name: None,
                },
            ),
            "cellular" => self.presence(
                id,
                Presence::Status {
                    battery_level: None,
                    battery_charging: None,
                    signal_strength: num("signal"),
                    carrier_name: ev.get("carrier").and_then(Value::as_str).map(str::to_string),
                },
            ),
            "call" => {
                let phase = match ev.get("phase").and_then(Value::as_str) {
                    Some("ringing") => CallPhase::Ringing,
                    Some("active") => CallPhase::Active,
                    Some("ended") => CallPhase::Ended,
                    _ => return,
                };
                self.emit(CpEvent::Call { id, phase });
            }
            "albumart" | "navigation" | "nowplaying" => {
                self.emit(CpEvent::Metadata { id, event: ev.clone() });
            }
            _ => {}
        }
    }

    fn on_cmd(&mut self, cmd: CpCmd) {
        match cmd {
            CpCmd::Stack { id, cmd } => {
                if let Some(s) = self.sessions.get(&id) {
                    s.send(cmd);
                }
            }
            CpCmd::NightMode(night) => {
                for s in self.sessions.values() {
                    s.send(StackCmd::NightMode(night));
                }
            }
            CpCmd::InitialNightMode(night) => {
                self.initial_night_mode = night;
                if let Some(night) = night {
                    for s in self.sessions.values() {
                        s.send(StackCmd::NightMode(night));
                    }
                }
            }
            CpCmd::LimitedUi(limited) => {
                for s in self.sessions.values() {
                    s.send(StackCmd::LimitedUi(limited));
                }
            }
            CpCmd::ClusterActive(active) => {
                self.cluster_stream_active = active;
                for s in self.sessions.values() {
                    s.send(StackCmd::ClusterActive(active));
                }
            }
            CpCmd::DropSessions => {
                for id in self.sessions.keys().copied().collect::<Vec<_>>() {
                    self.close(id);
                }
                let helper = self.ctx.helper.clone();
                tokio::spawn(async move {
                    let _ = helper.drop_iap2().await;
                });
            }
            CpCmd::Close { id } => self.close(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::net::unix::OwnedWriteHalf;
    use tokio::sync::watch;

    use super::*;
    use crate::bplist::{Value as Plist, dict};
    use crate::crypto::{ed25519_public, random_bytes};
    use crate::helper_sock::HelperSock;
    use crate::identity::load_or_create;
    use crate::net;
    use crate::pairings::Pairings;
    use crate::pairings::tests::TempDir;
    use crate::stack::StackConfig;
    use crate::stack::tests::{FakeMedia, Phone};

    struct FakeHelper {
        lines: Arc<Mutex<Vec<String>>>,
        subscriber: Arc<tokio::sync::Mutex<Option<OwnedWriteHalf>>>,
    }

    impl FakeHelper {
        fn start(path: &Path) -> Self {
            let listener = UnixListener::bind(path).unwrap();
            let lines = Arc::new(Mutex::new(Vec::new()));
            let subscriber = Arc::new(tokio::sync::Mutex::new(None));
            let (log, sub) = (lines.clone(), subscriber.clone());
            tokio::spawn(async move {
                while let Ok((sock, _)) = listener.accept().await {
                    let (rd, mut wr) = sock.into_split();
                    let mut reader = BufReader::new(rd);
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.is_err() {
                        continue;
                    }
                    let line = line.trim().to_string();
                    if line == "subscribe" {
                        *sub.lock().await = Some(wr);
                        continue;
                    }
                    log.lock().unwrap().push(line);
                    let _ = wr.write_all(b"{\"ok\":true}\n").await;
                }
            });
            Self { lines, subscriber }
        }

        async fn push(&self, event: serde_json::Value) {
            for _ in 0..200 {
                if let Some(wr) = self.subscriber.lock().await.as_mut() {
                    wr.write_all(format!("{event}\n").as_bytes()).await.unwrap();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("nobody subscribed");
        }

        async fn saw(&self, line: &str) {
            for _ in 0..300 {
                if self.lines.lock().unwrap().iter().any(|l| l == line) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the helper never got {line:?}: {:?}", self.lines.lock().unwrap());
        }
    }

    struct Rig {
        _dir: TempDir,
        helper: FakeHelper,
        handle: CpHandle,
        events: mpsc::UnboundedReceiver<CpEvent>,
        port: u16,
        secret: [u8; 32],
    }

    impl Rig {
        async fn new() -> Self {
            let dir = TempDir::new();
            let identity = load_or_create(&dir.0.join("identity.json"));
            let pairings = Pairings::new(dir.0.join("pairings.json"));
            let secret: [u8; 32] = random_bytes();
            pairings.save("phone-1", &ed25519_public(&secret));
            let path = dir.0.join("cp.sock");
            let helper = FakeHelper::start(&path);
            let (tx, config) = watch::channel(StackConfig {
                info: crate::info::tests::sample(),
                audio_device: String::new(),
                audio_input_device: String::new(),
            });
            std::mem::forget(tx);
            let ctx = Arc::new(StackCtx {
                identity,
                pairings,
                helper: HelperSock::new(&path),
                media: FakeMedia::new(),
                config,
                refresh: None,
                debug: false,
                video_playback: false,
                video_capture: None,
            });
            let listener = net::tcp_listener().unwrap();
            let port = net::local_port(listener.local_addr());
            let (ev_tx, events) = mpsc::unbounded_channel();
            let handle = start(listener, ctx, ev_tx);
            let mut rig = Self { _dir: dir, helper, handle, events, port, secret };
            assert_eq!(rig.event().await, CpEvent::HelperConnected);
            rig
        }

        async fn event(&mut self) -> CpEvent {
            tokio::time::timeout(Duration::from_secs(5), self.events.recv()).await.unwrap().unwrap()
        }

        async fn phone(&mut self, bt: &str, wifi: &str) -> (Phone, SessionId) {
            let sock = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
            let spawned = self.until(|e| matches!(e, CpEvent::Spawned { .. })).await;
            let CpEvent::Spawned { id } = spawned else { unreachable!() };
            let mut phone = Phone::new(sock);
            phone.verify(&self.secret, b"phone-1").await;
            let session = dict([
                ("deviceID", Plist::String(bt.into())),
                ("macAddress", Plist::String(wifi.into())),
                ("name", Plist::String("iPhone".into())),
            ]);
            phone.plist("SETUP", "/s", &session).await;
            (phone, id)
        }

        async fn screen(&mut self, phone: &mut Phone) {
            let streams = dict([(
                "streams",
                Plist::Array(vec![dict([
                    ("type", Plist::Int(110)),
                    ("streamConnectionID", Plist::Int(1)),
                ])]),
            )]);
            phone.plist("SETUP", "/s", &streams).await;
        }

        async fn until(&mut self, want: impl Fn(&CpEvent) -> bool) -> CpEvent {
            loop {
                let ev = self.event().await;
                if want(&ev) {
                    return ev;
                }
            }
        }
    }

    fn device(bt: &str, wifi: &str, ip: &str, usb: &str) -> Presence {
        Presence::Device {
            bt_mac: bt.into(),
            wifi_mac: wifi.into(),
            ip: ip.into(),
            usb_udid: usb.into(),
            name: if ip.is_empty() { "iPhone".into() } else { String::new() },
            model: String::new(),
        }
    }

    #[tokio::test]
    async fn a_session_takes_its_identity_and_its_metadata() {
        let mut rig = Rig::new().await;
        rig.helper
            .push(serde_json::json!({ "type": "device", "btMac": "AA:BB", "usbUdid": "u1", "ip": "10.0.0.2" }))
            .await;
        let ids = DeviceIds {
            bt_mac: Some("AA:BB".into()),
            ip: Some("10.0.0.2".into()),
            usb_udid: Some("u1".into()),
            ..Default::default()
        };
        assert_eq!(rig.event().await, CpEvent::HelperPresence(HelperPresence::Device { ids }));

        let (mut phone, id) = rig.phone("AA:BB", "CC:DD").await;
        let mut adopted = device("AA:BB", "cc:dd", "10.0.0.2", "u1");
        if let Presence::Device { name, .. } = &mut adopted {
            name.clear();
        }
        assert_eq!(rig.event().await, CpEvent::Presence { id, wired: false, presence: adopted });
        assert_eq!(
            rig.event().await,
            CpEvent::Presence { id, wired: false, presence: device("AA:BB", "cc:dd", "", "") }
        );
        rig.screen(&mut phone).await;
        assert_eq!(
            rig.event().await,
            CpEvent::VideoCodec { id, cluster: false, codec: ScreenCodec::H265 }
        );
        assert_eq!(rig.event().await, CpEvent::Connected { id, wired: false, controller_id: None });
        phone.request("RECORD", "/s", b"").await;
        assert_eq!(
            rig.event().await,
            CpEvent::Presence {
                id,
                wired: false,
                presence: Presence::Active { ip: "127.0.0.1".into() }
            }
        );
        rig.helper.saw("tunnel phone-1 AA:BB").await;

        rig.helper
            .push(serde_json::json!({ "type": "nowplaying", "phoneId": "AA:BB", "title": "x" }))
            .await;
        assert!(matches!(rig.event().await, CpEvent::Metadata { id: m, .. } if m == id));
        rig.helper.push(serde_json::json!({ "type": "call", "phase": "ringing" })).await;
        assert_eq!(rig.event().await, CpEvent::Call { id, phase: CallPhase::Ringing });
        rig.helper
            .push(serde_json::json!({ "type": "call", "cid": "phone-1", "phase": "active" }))
            .await;
        assert_eq!(rig.event().await, CpEvent::Call { id, phase: CallPhase::Active });
        rig.helper.push(serde_json::json!({ "type": "call", "phase": "ended" })).await;
        assert_eq!(rig.event().await, CpEvent::Call { id, phase: CallPhase::Ended });
        rig.helper.push(serde_json::json!({ "type": "call", "phase": "odd" })).await;
        rig.helper
            .push(serde_json::json!({ "type": "power", "level": 50, "charging": true }))
            .await;
        assert_eq!(
            rig.event().await,
            CpEvent::Presence {
                id,
                wired: false,
                presence: Presence::Status {
                    battery_level: Some(50.0),
                    battery_charging: Some(true),
                    signal_strength: None,
                    carrier_name: None,
                },
            }
        );
        rig.helper
            .push(serde_json::json!({ "type": "cellular", "signal": 3, "carrier": "Tel" }))
            .await;
        assert!(matches!(
            rig.event().await,
            CpEvent::Presence { presence: Presence::Status { .. }, .. }
        ));
        rig.helper.push(serde_json::json!({ "type": "deviceTime" })).await;
        rig.helper.push(serde_json::json!({ "type": "deviceTime", "utcOffsetMinutes": 120 })).await;
        assert_eq!(rig.event().await, CpEvent::PhoneUtcOffset { minutes: 120 });
        rig.helper.push(serde_json::json!({ "type": "unknown" })).await;

        rig.handle.stack(id, StackCmd::Keyframe);
        rig.handle.send(CpCmd::NightMode(true));
        rig.handle.send(CpCmd::InitialNightMode(Some(false)));
        rig.handle.send(CpCmd::LimitedUi(true));
        rig.handle.send(CpCmd::ClusterActive(false));
        phone.sock.shutdown().await.unwrap();
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Disconnected { .. })).await,
            CpEvent::Disconnected { id }
        );

        rig.helper.push(serde_json::json!({ "type": "nowplaying", "phoneId": "EE:FF" })).await;
        let CpEvent::Spawned { id: meta } = rig.event().await else { panic!("no meta session") };
        assert_eq!(
            rig.event().await,
            CpEvent::Presence {
                id: meta,
                wired: false,
                presence: Presence::Device {
                    bt_mac: "EE:FF".into(),
                    wifi_mac: String::new(),
                    ip: String::new(),
                    usb_udid: String::new(),
                    name: String::new(),
                    model: String::new(),
                },
            }
        );
        assert!(matches!(rig.event().await, CpEvent::Metadata { id: m, .. } if m == meta));
        rig.helper.push(serde_json::json!({ "type": "nowplaying", "phoneId": "AA:BB" })).await;
        let again = rig.until(|e| matches!(e, CpEvent::Spawned { .. })).await;
        let CpEvent::Spawned { id: again } = again else { unreachable!() };
        rig.helper.push(serde_json::json!({ "type": "device-gone", "usbUdid": "u1" })).await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::HelperPresence(_))).await,
            CpEvent::HelperPresence(HelperPresence::DeviceGone { usb_udid: "u1".into() })
        );
        assert_eq!(rig.event().await, CpEvent::Disconnected { id: again });
        rig.handle.send(CpCmd::DropSessions);
        assert_eq!(rig.event().await, CpEvent::Disconnected { id: meta });
        rig.helper.saw("drop-iap2").await;
    }

    #[tokio::test]
    async fn the_cable_takes_over_from_wireless() {
        let mut rig = Rig::new().await;
        let (mut phone, id) = rig.phone("GG:HH", "11:22").await;
        rig.screen(&mut phone).await;
        phone.request("RECORD", "/s", b"").await;
        rig.until(|e| matches!(e, CpEvent::Presence { presence: Presence::Active { .. }, .. }))
            .await;

        rig.helper
            .push(serde_json::json!({ "type": "device", "btMac": "GG:HH", "usbUdid": "u3" }))
            .await;
        rig.until(|e| matches!(e, CpEvent::Presence { presence: Presence::Device { usb_udid, .. }, .. } if usb_udid == "u3")).await;
        rig.helper
            .push(serde_json::json!({ "type": "wired-start", "usbUdid": "u3", "ip": "::ffff:127.0.0.1", "phoneId": "GG:HH" }))
            .await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Disconnected { .. })).await,
            CpEvent::Disconnected { id }
        );
        rig.helper.saw("start-wired u3").await;

        let (mut wired, wired_id) = rig.phone("GG:HH", "").await;
        rig.screen(&mut wired).await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Connected { .. })).await,
            CpEvent::Connected { id: wired_id, wired: true, controller_id: None }
        );
        // A wifi leave and a lost link do not end the session on the cable. The
        // cable leaving does, once the phone identified on it.
        rig.helper
            .push(serde_json::json!({ "type": "wifi", "mac": "11:22", "event": "left" }))
            .await;
        rig.helper.push(serde_json::json!({ "type": "link", "up": false })).await;
        rig.helper
            .push(serde_json::json!({ "type": "device", "btMac": "GG:HH", "usbUdid": "u3" }))
            .await;
        rig.helper.push(serde_json::json!({ "type": "device-gone", "usbUdid": "u3" })).await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Disconnected { .. })).await,
            CpEvent::Disconnected { id: wired_id }
        );
    }

    #[tokio::test]
    async fn wireless_ends_with_the_access_point_or_the_link() {
        let mut rig = Rig::new().await;
        let (_a, a) = rig.phone("A1:A1", "aa:01").await;
        let (_b, b) = rig.phone("B1:B1", "bb:01").await;
        rig.helper.push(serde_json::json!({ "type": "wifi", "mac": "aa:01", "event": "left", "ip": "1.2.3.4" })).await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Disconnected { .. })).await,
            CpEvent::Disconnected { id: a }
        );
        // The phone that left is gone, its last metadata makes no session.
        rig.helper.push(serde_json::json!({ "type": "nowplaying", "phoneId": "A1:A1" })).await;
        rig.helper
            .push(serde_json::json!({ "type": "wifi", "mac": "aa:02", "event": "joined" }))
            .await;
        assert_eq!(
            rig.until(|e| matches!(
                e,
                CpEvent::HelperPresence(HelperPresence::Wifi { connected: true, .. })
            ))
            .await,
            CpEvent::HelperPresence(HelperPresence::Wifi {
                wifi_mac: "aa:02".into(),
                ip: String::new(),
                connected: true
            })
        );
        // Untagged metadata with two sessions belongs to nobody.
        rig.helper.push(serde_json::json!({ "type": "device", "usbUdid": "u9" })).await;
        rig.helper.push(serde_json::json!({ "type": "link", "up": false })).await;
        assert_eq!(
            rig.until(|e| matches!(e, CpEvent::Disconnected { .. })).await,
            CpEvent::Disconnected { id: b }
        );
        rig.handle.send(CpCmd::Close { id: 99 });
        drop(rig.handle);
    }
}
