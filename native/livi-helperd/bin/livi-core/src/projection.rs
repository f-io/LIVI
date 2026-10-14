use std::sync::Arc;
use std::time::Duration;

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use livi_aa_stack::aa_session::SessionCmd;
use livi_aa_stack::bridge::{Navigation as AaNavigation, NowPlaying as AaNowPlaying};
use livi_aa_stack::commands::{Command, TouchAction, TouchItem};
use livi_aa_stack::manager::{AaCmd, AaEvent, AaHandle, SessionId as AaId};
use livi_aa_stack::media::AudioKind as AaKind;
use livi_core_proto::config::{Config, KeyBindings};
use livi_core_proto::input::{Input, Phase, Point};
use livi_core_proto::message::{MediaControl, Radio};
use livi_core_proto::state::{
    Front, Navigation, NowPlaying, PerScreen, Protocol, Screen, Sessions, State,
};
use livi_cp::helper_sock::HelperSock;
use livi_cp::hid::{KnobState, media_button, telephony_button};
use livi_cp::manager::{CallPhase, CpCmd, CpEvent, CpHandle, HelperPresence, Presence, SessionId};
use livi_cp::stack::{AudioKind, StackCmd, Touch};
use livi_media::planes::{ClusterPlanes, Crop, MainPlane};
use serde_json::{Map, Value};
use tokio::sync::{Notify, broadcast, mpsc, oneshot, watch};

use crate::aa_telemetry::{self, AaTelemetry};
use crate::android_auto;
use crate::carplay::night_mode;
use crate::cp_telemetry::{CpTelemetry, FROM_CARPLAY};
use crate::devices::{self, Devices, Seen};
use crate::hands_free::{self, Back, HandsFree, Io};
use crate::helper;
use crate::hub::Hub;
use crate::nav_text;
use crate::spectrum;
use crate::status_file::Activity;
use crate::telemetry::{CLIMATE, SOURCE};
use crate::update::UpdateAsk;

const RAMP_DOWN_MS: u32 = 500;
const RAMP_UP_MS: u32 = 1500;
const KNOB_DEFLECT: f64 = 127.0;
/// The goodbye is best effort, the session of a forgotten phone ends after it.
const FORGET_GRACE: Duration = Duration::from_millis(1500);
/// A bound select key is a click, the release follows on its own.
const SELECT_RELEASE: Duration = Duration::from_millis(200);
/// A typed name or a picker sends several changes in a row.
const APPLY_AFTER_QUIET: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Usb,
    Wifi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Cp(SessionId),
    Aa(AaId),
}

impl Link {
    fn protocol(self) -> Protocol {
        match self {
            Link::Cp(_) => Protocol::Carplay,
            Link::Aa(_) => Protocol::Androidauto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Do {
    Keyframe,
    Home,
    Media(MediaControl),
    Volume { kind: AudioKind, level: f64, ramp_ms: u32 },
    VideoActive(bool),
    Close,
}

fn media_index(control: MediaControl) -> u8 {
    match control {
        MediaControl::Play => media_button::PLAY,
        MediaControl::Pause => media_button::PAUSE,
        MediaControl::PlayPause => media_button::PLAY_PAUSE,
        MediaControl::Next => media_button::NEXT,
        MediaControl::Prev => media_button::PREV,
    }
}

fn media_command(control: MediaControl) -> Command {
    match control {
        MediaControl::Play => Command::Play,
        MediaControl::Pause => Command::Pause,
        MediaControl::PlayPause => Command::PlayPause,
        MediaControl::Next => Command::Next,
        MediaControl::Prev => Command::Prev,
    }
}

fn aa_kind(kind: AudioKind) -> AaKind {
    match kind {
        AudioKind::Media => AaKind::Media,
        AudioKind::Alert => AaKind::Alert,
        AudioKind::Speech => AaKind::Speech,
        AudioKind::Call => AaKind::Call,
    }
}

fn cp_kind(kind: AaKind) -> AudioKind {
    match kind {
        AaKind::Media => AudioKind::Media,
        AaKind::Alert => AudioKind::Alert,
        AaKind::Speech => AudioKind::Speech,
        AaKind::Call => AudioKind::Call,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ids {
    pub bt_mac: Option<String>,
    pub wifi_mac: Option<String>,
    pub usb_udid: Option<String>,
    pub usb_serial: Option<String>,
    pub instance_id: Option<String>,
    pub controller_id: Option<String>,
    pub ip: Option<String>,
}

impl Ids {
    fn any(&self) -> bool {
        self.bt_mac.is_some()
            || self.wifi_mac.is_some()
            || self.usb_udid.is_some()
            || self.usb_serial.is_some()
            || self.instance_id.is_some()
            || self.controller_id.is_some()
            || self.ip.is_some()
    }

    fn overlaps(&self, other: &Ids) -> bool {
        let mac = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
        let same =
            |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a == b);
        mac(&self.bt_mac, &other.bt_mac)
            || mac(&self.wifi_mac, &other.wifi_mac)
            || same(&self.usb_udid, &other.usb_udid)
            || same(&self.usb_serial, &other.usb_serial)
            || same(&self.instance_id, &other.instance_id)
            || same(&self.controller_id, &other.controller_id)
            || same(&self.ip, &other.ip)
    }

    fn merge(&mut self, from: &Ids) {
        let take = |into: &mut Option<String>, v: &Option<String>, lower: bool| {
            if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                *into = Some(if lower { v.to_lowercase() } else { v.clone() });
            }
        };
        take(&mut self.bt_mac, &from.bt_mac, true);
        take(&mut self.wifi_mac, &from.wifi_mac, true);
        take(&mut self.usb_udid, &from.usb_udid, false);
        take(&mut self.usb_serial, &from.usb_serial, false);
        take(&mut self.instance_id, &from.instance_id, false);
        take(&mut self.controller_id, &from.controller_id, false);
        take(&mut self.ip, &from.ip, false);
    }
}

fn opt(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    index: u32,
    link: Link,
    transport: Transport,
    device: Ids,
    active: bool,
    duck_level: f64,
    duck_ramp_ms: u32,
    /// Kept per phone, so a held one comes back with its own.
    now_playing: NowPlaying,
    navigation: Navigation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Levels {
    music: f64,
    nav: f64,
    voice: f64,
    call: f64,
}

impl Levels {
    fn from(cfg: &Config) -> Self {
        Self {
            music: cfg.audio_volume,
            nav: cfg.nav_volume,
            voice: cfg.voice_assistant_volume,
            call: cfg.call_volume,
        }
    }

    fn get(&self, kind: AudioKind) -> f64 {
        match kind {
            AudioKind::Media => self.music,
            AudioKind::Alert => self.nav,
            AudioKind::Speech => self.voice,
            AudioKind::Call => self.call,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attention {
    Call,
    VoiceAssistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Left,
    Right,
    SelectUp,
    SelectDown,
    Back,
    KnobLeft,
    KnobRight,
    KnobUp,
    KnobDown,
    Home,
    CycleSession,
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
    AcceptPhone,
    RejectPhone,
    Phone(u8),
    VoiceAssistant,
    VoiceAssistantRelease,
}

impl Key {
    fn works_behind(self) -> bool {
        matches!(
            self,
            Key::Home
                | Key::PlayPause
                | Key::Play
                | Key::Pause
                | Key::Next
                | Key::Prev
                | Key::AcceptPhone
                | Key::RejectPhone
                | Key::VoiceAssistant
        )
    }
}

fn bound(b: &KeyBindings, code: &str) -> Option<Key> {
    use telephony_button as t;
    let table = [
        (&b.up, Key::Up),
        (&b.down, Key::Down),
        (&b.left, Key::Left),
        (&b.right, Key::Right),
        (&b.select_up, Key::SelectUp),
        (&b.select_down, Key::SelectDown),
        (&b.back, Key::Back),
        (&b.knob_left, Key::KnobLeft),
        (&b.knob_right, Key::KnobRight),
        (&b.knob_up, Key::KnobUp),
        (&b.knob_down, Key::KnobDown),
        (&b.home, Key::Home),
        (&b.cycle_session, Key::CycleSession),
        (&b.play_pause, Key::PlayPause),
        (&b.play, Key::Play),
        (&b.pause, Key::Pause),
        (&b.next, Key::Next),
        (&b.prev, Key::Prev),
        (&b.accept_phone, Key::AcceptPhone),
        (&b.reject_phone, Key::RejectPhone),
        (&b.phone_key0, Key::Phone(t::KEY0)),
        (&b.phone_key1, Key::Phone(t::KEY0 + 1)),
        (&b.phone_key2, Key::Phone(t::KEY0 + 2)),
        (&b.phone_key3, Key::Phone(t::KEY0 + 3)),
        (&b.phone_key4, Key::Phone(t::KEY0 + 4)),
        (&b.phone_key5, Key::Phone(t::KEY0 + 5)),
        (&b.phone_key6, Key::Phone(t::KEY0 + 6)),
        (&b.phone_key7, Key::Phone(t::KEY0 + 7)),
        (&b.phone_key8, Key::Phone(t::KEY0 + 8)),
        (&b.phone_key9, Key::Phone(t::KEY0 + 9)),
        (&b.phone_key_star, Key::Phone(t::STAR)),
        (&b.phone_key_hash, Key::Phone(t::POUND)),
        (&b.phone_key_hook_switch, Key::Phone(t::HOOK_SWITCH)),
        (&b.voice_assistant, Key::VoiceAssistant),
        (&b.voice_assistant_release, Key::VoiceAssistantRelease),
    ];
    table.into_iter().find(|(c, _)| !c.is_empty() && c.as_str() == code).map(|(_, key)| key)
}

/// None while the car said neither.
fn reversing(telemetry: &Map<String, Value>) -> Option<bool> {
    match telemetry.get("reverse").and_then(Value::as_bool) {
        Some(reverse) => Some(reverse),
        None => telemetry
            .get("gear")
            .map(|g| aa_telemetry::gear(Some(g), None) == Some(aa_telemetry::REVERSE)),
    }
}

fn shows_cluster(wants: &PerScreen<Front>) -> bool {
    [wants.main, wants.cluster, wants.aux].contains(&Front::Cluster)
}

fn session_ids(ids: &devices::Ids) -> Ids {
    Ids {
        bt_mac: ids.bt_mac.clone(),
        wifi_mac: ids.wifi_mac.clone(),
        usb_udid: ids.usb_udid.clone(),
        usb_serial: ids.usb_serial.clone(),
        instance_id: ids.instance_id.clone(),
        controller_id: None,
        ip: ids.ip.clone(),
    }
}

fn registry_ids(ids: &Ids) -> devices::Ids {
    devices::Ids {
        bt_mac: ids.bt_mac.clone(),
        wifi_mac: ids.wifi_mac.clone(),
        usb_udid: ids.usb_udid.clone(),
        usb_serial: ids.usb_serial.clone(),
        instance_id: ids.instance_id.clone(),
        ip: ids.ip.clone(),
    }
}

fn knob(state: KnobState) -> StackCmd {
    StackCmd::Knob { state, momentary: true }
}

pub struct Drivers {
    pub cp: CpHandle,
    pub aa: AaHandle,
    pub aa_crop: watch::Receiver<Option<Crop>>,
    pub phone_io: mpsc::UnboundedSender<Io>,
    pub phone_back: mpsc::UnboundedReceiver<Back>,
    /// Into the telemetry, like any producer inside core.
    pub telemetry: mpsc::UnboundedSender<Value>,
}

pub struct Projection {
    hub: Arc<Hub>,
    cp: CpHandle,
    aa: AaHandle,
    aa_crop: watch::Receiver<Option<Crop>>,
    phone_io: mpsc::UnboundedSender<Io>,
    phone_back: Option<mpsc::UnboundedReceiver<Back>>,
    telemetry: mpsc::UnboundedSender<Value>,
    hands_free: HandsFree,
    /// Wired and USB serial of each Android Auto session until it projects.
    aa_spawned: HashMap<AaId, (bool, Option<String>)>,
    plane: Arc<MainPlane>,
    clusters: Arc<ClusterPlanes>,
    helper_restart: Arc<Notify>,
    devices: Devices,
    cluster_on: bool,
    sessions: Vec<Entry>,
    next_index: u32,
    video_owner: Option<Link>,
    levels: Levels,
    duck_level: f64,
    duck_ramp_ms: u32,
    wants: PerScreen<Front>,
    /// What the UI last reported, so only its own changes override core's.
    asked: PerScreen<Front>,
    /// The kind holding the projection in front and the view main returns to.
    attention: Option<(Attention, Front)>,
    bindings: KeyBindings,
    /// Telemetry reaches the state many times a second, the config rarely.
    config_seen: Config,
    telemetry_seen: Map<String, Value>,
    reversing: bool,
    camera_found: bool,
    cp_telemetry: CpTelemetry,
    aa_telemetry: AaTelemetry,
    bye: Option<oneshot::Sender<()>>,
    activity: Option<watch::Sender<Activity>>,
    apply_due: Option<tokio::time::Instant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    Media(MediaControl),
    Seek(u32),
    NextDevice,
    ApplySettings,
    SelectDevice(String),
    ForgetDevice(String),
    CameraFound(bool),
}

pub struct UiAsks {
    pub wants: watch::Receiver<PerScreen<Front>>,
    pub commands: mpsc::UnboundedReceiver<UiCommand>,
    pub input: mpsc::UnboundedReceiver<Input>,
    pub spectrum: Arc<spectrum::Feed>,
    pub goodbye: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    pub activity: watch::Sender<Activity>,
}

pub struct UiSenders {
    pub wants: watch::Sender<PerScreen<Front>>,
    pub commands: mpsc::UnboundedSender<UiCommand>,
    pub input: mpsc::UnboundedSender<Input>,
    pub dongle: mpsc::UnboundedSender<(Radio, bool)>,
    pub link_speed_viewers: watch::Sender<usize>,
    pub spectrum: Arc<spectrum::Feed>,
    pub goodbye: mpsc::UnboundedSender<oneshot::Sender<()>>,
    pub update: mpsc::UnboundedSender<UpdateAsk>,
    pub activity: watch::Receiver<Activity>,
}

pub struct DongleAsks {
    pub radios: mpsc::UnboundedReceiver<(Radio, bool)>,
    pub link_speed_viewers: watch::Receiver<usize>,
}

pub type UpdateAsks = mpsc::UnboundedReceiver<UpdateAsk>;

pub fn ui_channels(wants: PerScreen<Front>) -> (UiSenders, UiAsks, DongleAsks, UpdateAsks) {
    let (wants_tx, wants_rx) = watch::channel(wants);
    let (commands_tx, commands_rx) = mpsc::unbounded_channel();
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (dongle_tx, dongle_rx) = mpsc::unbounded_channel();
    let (viewers_tx, viewers_rx) = watch::channel(0);
    let (goodbye_tx, goodbye_rx) = mpsc::unbounded_channel();
    let (update_tx, update_rx) = mpsc::unbounded_channel();
    let (activity_tx, activity_rx) = watch::channel(Activity::default());
    let feed = spectrum::Feed::new();
    (
        UiSenders {
            wants: wants_tx,
            commands: commands_tx,
            input: input_tx,
            dongle: dongle_tx,
            link_speed_viewers: viewers_tx,
            spectrum: feed.clone(),
            goodbye: goodbye_tx,
            update: update_tx,
            activity: activity_rx,
        },
        UiAsks {
            wants: wants_rx,
            commands: commands_rx,
            input: input_rx,
            spectrum: feed,
            goodbye: goodbye_rx,
            activity: activity_tx,
        },
        DongleAsks { radios: dongle_rx, link_speed_viewers: viewers_rx },
        update_rx,
    )
}

impl Projection {
    pub fn new(
        hub: Arc<Hub>,
        drivers: Drivers,
        plane: Arc<MainPlane>,
        clusters: Arc<ClusterPlanes>,
        helper_restart: Arc<Notify>,
        devices: Devices,
        wants: PerScreen<Front>,
    ) -> Self {
        let Drivers { cp, aa, aa_crop, phone_io, phone_back, telemetry } = drivers;
        let cfg = hub.config();
        cp.send(CpCmd::InitialNightMode(night_mode(cfg.appearance_mode)));
        let cluster_on = shows_cluster(&wants);
        cp.send(CpCmd::ClusterActive(cluster_on));
        aa.send(AaCmd::ClusterActive(cluster_on));
        Self {
            cp,
            aa,
            aa_crop,
            phone_io,
            phone_back: Some(phone_back),
            telemetry,
            hands_free: HandsFree::default(),
            aa_spawned: HashMap::new(),
            plane,
            clusters,
            helper_restart,
            devices,
            cluster_on,
            sessions: Vec::new(),
            next_index: 1,
            video_owner: None,
            levels: Levels::from(&cfg),
            duck_level: 1.0,
            duck_ramp_ms: RAMP_UP_MS,
            wants,
            asked: wants,
            attention: None,
            bindings: cfg.bindings.clone(),
            telemetry_seen: hub.watch().borrow().telemetry.clone(),
            reversing: false,
            camera_found: false,
            config_seen: cfg,
            cp_telemetry: CpTelemetry::new(HelperSock::default()),
            aa_telemetry: AaTelemetry::default(),
            bye: None,
            activity: None,
            apply_due: None,
            hub,
        }
    }

    pub async fn run(
        mut self,
        mut events: mpsc::UnboundedReceiver<CpEvent>,
        mut aa_events: mpsc::UnboundedReceiver<AaEvent>,
        mut asks: UiAsks,
        mut state: watch::Receiver<State>,
    ) {
        let mut created = Some(self.plane.player_created());
        let mut cluster_created = Some(self.clusters.player_created());
        let mut back = self.phone_back.take().unwrap_or_else(|| mpsc::unbounded_channel().1);
        self.activity = Some(asks.activity);
        let mut keep = tokio::time::interval(hands_free::KEEP_EVERY);
        keep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(ev) => self.on_event(ev),
                    None => return,
                },
                Some(ev) = aa_events.recv() => self.on_aa(ev),
                Some(answer) = back.recv() => self.on_back(answer),
                _ = keep.tick() => self.keep_links(),
                changed = asks.wants.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let wants = *asks.wants.borrow_and_update();
                    self.asked(wants);
                }
                Some(command) = asks.commands.recv() => match command {
                    UiCommand::Media(control) => self.media(control),
                    UiCommand::Seek(ms) => self.seek(ms),
                    UiCommand::NextDevice => self.activate_next(),
                    UiCommand::ApplySettings => self.apply_settings(),
                    UiCommand::SelectDevice(id) => self.select_device(&id),
                    UiCommand::ForgetDevice(id) => self.forget_device(&id),
                    UiCommand::CameraFound(found) => self.camera_found(found),
                },
                Some(input) = asks.input.recv() => self.input(input),
                Some(done) = asks.goodbye.recv() => self.goodbye(done),
                () = until(self.apply_due) => self.apply_idle(),
                changed = state.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let (cfg, telemetry) = {
                        let s = state.borrow_and_update();
                        (s.config.clone(), s.telemetry.clone())
                    };
                    self.on_state(cfg, telemetry);
                }
                made = player_created(&mut created) => match made {
                    Ok(()) => self.keyframe(),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => created = None,
                },
                made = player_created(&mut cluster_created) => match made {
                    Ok(()) => self.keyframe(),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => cluster_created = None,
                },
            }
        }
    }

    fn active(&self) -> Option<&Entry> {
        self.sessions.iter().find(|s| s.active)
    }

    fn is_active(&self, link: Link) -> bool {
        self.active().is_some_and(|s| s.link == link)
    }

    fn to_active(&self, what: Do) {
        if let Some(s) = self.active() {
            self.tell(s.link, what);
        }
    }

    fn tell(&self, link: Link, what: Do) {
        match link {
            Link::Cp(id) => match what {
                Do::Keyframe => self.cp.stack(id, StackCmd::Keyframe),
                Do::Home => self.cp.stack(id, knob(KnobState { home: true, ..Default::default() })),
                Do::Media(control) => self.cp.stack(id, StackCmd::Media(media_index(control))),
                Do::Volume { kind, level, ramp_ms } => {
                    self.cp.stack(id, StackCmd::StreamVolume { kind, level, ramp_ms });
                }
                Do::VideoActive(on) => {
                    self.cp.stack(id, StackCmd::VideoActive(on));
                    self.cp.stack(id, StackCmd::AudioActive(on));
                }
                Do::Close => self.cp.send(CpCmd::Close { id }),
            },
            Link::Aa(id) => match what {
                Do::Keyframe => self.aa.session(id, SessionCmd::Keyframe),
                Do::Home => self.aa.session(id, SessionCmd::Command(Command::Home)),
                Do::Media(control) => {
                    self.aa.session(id, SessionCmd::Command(media_command(control)));
                }
                Do::Volume { kind, level, ramp_ms } => {
                    self.aa.session(
                        id,
                        SessionCmd::StreamVolume { kind: aa_kind(kind), level, ramp_ms },
                    );
                }
                Do::VideoActive(on) => self.aa.session(id, SessionCmd::VideoActive(on)),
                Do::Close => self.aa.send(AaCmd::Close { id }),
            },
        }
    }

    fn by_link(&mut self, link: Link) -> Option<&mut Entry> {
        self.sessions.iter_mut().find(|s| s.link == link)
    }

    fn by_identity(&self, ids: &Ids) -> Option<usize> {
        if !ids.any() {
            return None;
        }
        self.sessions.iter().position(|s| s.device.overlaps(ids))
    }

    fn on_event(&mut self, ev: CpEvent) {
        match ev {
            CpEvent::Connected { id, wired, controller_id } => {
                let ids = Ids { controller_id, ..Default::default() };
                let index = self.upsert(Link::Cp(id), transport(wired), &ids);
                self.auto_activate(index);
            }
            CpEvent::Presence { id, wired, presence } => self.presence(id, wired, presence),
            CpEvent::Disconnected { id } => self.close_by_link(Link::Cp(id)),
            CpEvent::AudioActive { id, profile, active } => {
                println!(
                    "[core] CarPlay audio {} {}",
                    profile.label,
                    if active { "started" } else { "stopped" }
                );
                // A stream opens at the host's default, it takes its level once it plays.
                if active && self.is_active(Link::Cp(id)) {
                    self.push_level(profile.kind, 0);
                }
                if self.is_active(Link::Cp(id)) {
                    self.activity(|a| a.stream(profile.kind, active));
                }
                if profile.kind == AudioKind::Call && self.is_active(Link::Cp(id)) {
                    self.attend(Attention::Call, active);
                }
            }
            CpEvent::Duck { id, level, duration_ms } => {
                self.on_duck(Link::Cp(id), level, duration_ms)
            }
            CpEvent::HelperPresence(HelperPresence::Wifi { wifi_mac, ip, connected }) => {
                let ids =
                    devices::Ids { wifi_mac: opt(&wifi_mac), ip: opt(&ip), ..Default::default() };
                self.devices.registry.note_wifi(&ids, connected);
                if !connected {
                    self.close_on_transport(&session_ids(&ids), Transport::Wifi);
                }
                self.publish();
            }
            CpEvent::HelperPresence(HelperPresence::Device { ids }) => {
                let found = devices::Ids {
                    bt_mac: ids.bt_mac,
                    usb_udid: ids.usb_udid,
                    ip: ids.ip,
                    ..Default::default()
                };
                // A phone plugged in during a wireless session still runs CarPlay over the air.
                let live = self.by_identity(&session_ids(&found));
                let wired = live.map_or(found.usb_udid.is_some(), |i| {
                    self.sessions[i].transport == Transport::Usb
                });
                let seen = Seen {
                    ids: found,
                    name: ids.name,
                    protocol: Some(Protocol::Carplay),
                    wired,
                    ..Default::default()
                };
                self.devices.registry.note_device(&seen);
                self.publish();
            }
            CpEvent::HelperConnected => {
                let targets = self.targets();
                self.devices.page(targets, true);
            }
            CpEvent::HelperPresence(HelperPresence::DeviceGone { usb_udid }) => {
                let ids = Ids { usb_udid: opt(&usb_udid), ..Default::default() };
                self.close_on_transport(&ids, Transport::Usb);
            }
            CpEvent::HostUiRequested { id } => {
                println!("[core] CarPlay asks for the car's own UI");
                if self.is_active(Link::Cp(id)) {
                    self.host_ui();
                }
            }
            CpEvent::SpeechActive { id, active } => {
                println!("[core] CarPlay Siri {}", if active { "active" } else { "done" });
                if self.is_active(Link::Cp(id)) {
                    self.attend(Attention::VoiceAssistant, active);
                }
            }
            CpEvent::Call { id, phase } => {
                println!("[core] CarPlay call {phase:?}");
                if self.is_active(Link::Cp(id)) {
                    self.attend(Attention::Call, phase != CallPhase::Ended);
                    self.activity(|a| a.phone = phase != CallPhase::Ended);
                }
            }
            CpEvent::Metadata { id, event } => self.metadata(Link::Cp(id), &event),
            CpEvent::Spawned { .. } => self.cp_telemetry.hydrate(&self.telemetry_seen, &self.cp),
            CpEvent::Climate { mut climate, .. } => {
                climate.insert(SOURCE.into(), FROM_CARPLAY.into());
                let _ = self.telemetry.send(serde_json::json!({ CLIMATE: climate }));
            }
            CpEvent::VideoCodec { .. } => {}
            CpEvent::PhoneUtcOffset { minutes } => {
                tokio::spawn(crate::gnss::timezone::apply_phone_offset(minutes));
            }
        }
    }

    fn presence(&mut self, id: SessionId, wired: bool, presence: Presence) {
        match presence {
            Presence::Device { bt_mac, wifi_mac, ip, usb_udid, name, model } => {
                let ids = Ids {
                    bt_mac: opt(&bt_mac),
                    wifi_mac: if wired { None } else { opt(&wifi_mac) },
                    usb_udid: opt(&usb_udid),
                    ip: opt(&ip),
                    ..Default::default()
                };
                let seen = Seen {
                    ids: devices::Ids {
                        bt_mac: ids.bt_mac.clone(),
                        wifi_mac: ids.wifi_mac.clone(),
                        usb_udid: ids.usb_udid.clone(),
                        ip: ids.ip.clone(),
                        ..Default::default()
                    },
                    name: opt(&name),
                    model: opt(&model),
                    protocol: Some(Protocol::Carplay),
                    wired,
                };
                self.devices.registry.note_device(&seen);
                if let Some(i) = self.by_identity(&ids)
                    && self.sessions[i].link != Link::Cp(id)
                {
                    let placeholder = self.sessions[i].link;
                    self.sessions[i].link = Link::Cp(id);
                    self.tell(placeholder, Do::Close);
                    self.changed();
                }
                let index = self.upsert(Link::Cp(id), transport(wired), &ids);
                self.auto_activate(index);
            }
            Presence::Active { .. } => {
                if let Some(index) = self.by_link(Link::Cp(id)).map(|s| s.index) {
                    self.auto_activate(index);
                }
            }
            Presence::Status { battery_level, battery_charging, signal_strength, carrier_name } => {
                let Some(ids) = self
                    .sessions
                    .iter()
                    .find(|s| s.link == Link::Cp(id))
                    .map(|s| registry_ids(&s.device))
                else {
                    return;
                };
                let status = devices::Status {
                    battery_level,
                    battery_charging,
                    signal_strength,
                    carrier_name,
                };
                self.devices.registry.note_status(&ids, &status);
                self.publish();
            }
        }
    }

    /// A wired driver taking over a wireless entry is the handover to the cable,
    /// the entry keeps its index.
    fn upsert(&mut self, link: Link, transport: Transport, device: &Ids) -> u32 {
        let by_identity = self.by_identity(device);
        let steals_wired = by_identity.is_some_and(|i| {
            let s = &self.sessions[i];
            s.link != link && s.transport == Transport::Usb && transport != Transport::Usb
        });
        let found = match by_identity {
            Some(i) if !steals_wired => Some(i),
            _ => self.sessions.iter().position(|s| s.link == link),
        };
        let mut superseded = None;
        let index = match found {
            Some(i) => {
                let s = &mut self.sessions[i];
                if s.link != link && transport == Transport::Usb && s.transport != Transport::Usb {
                    superseded = Some(s.link);
                    s.link = link;
                }
                s.transport = transport;
                s.device.merge(device);
                s.index
            }
            None => {
                let index = self.next_index;
                self.next_index += 1;
                let mut entry = Entry {
                    index,
                    link,
                    transport,
                    device: Ids::default(),
                    active: false,
                    duck_level: 1.0,
                    duck_ramp_ms: RAMP_UP_MS,
                    now_playing: NowPlaying::default(),
                    navigation: Navigation::default(),
                };
                entry.device.merge(device);
                self.sessions.push(entry);
                index
            }
        };
        if let Some(old) = superseded {
            self.tell(old, Do::Close);
        }
        self.changed();
        index
    }

    fn metadata(&mut self, link: Link, event: &Value) {
        let Some(entry) = self.by_link(link) else { return };
        let text = |key: &str| event.get(key).and_then(Value::as_str).map(str::to_string);
        let num = |key: &str| event.get(key).and_then(Value::as_f64);
        let code = |key: &str| num(key).map(|v| v as u32);
        match event.get("type").and_then(Value::as_str) {
            Some("navigation") => {
                let nav = &mut entry.navigation;
                if let Some(status) = num("status") {
                    if status == 0.0 {
                        *nav = Navigation::default();
                    }
                    nav.active = Some(status != 0.0);
                }
                for (field, key) in [
                    (&mut nav.road_name, "roadName"),
                    (&mut nav.after_road_name, "afterRoadName"),
                    (&mut nav.destination_name, "destinationName"),
                ] {
                    if let Some(v) = text(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut nav.time_to_destination, "timeToDestination"),
                    (&mut nav.distance_to_destination, "distanceToDestination"),
                    (&mut nav.remain_distance, "remainDistance"),
                ] {
                    if let Some(v) = num(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut nav.order_type, "orderType"),
                    (&mut nav.maneuver_type, "maneuverType"),
                    (&mut nav.turn_side, "turnSide"),
                    (&mut nav.junction_type, "junctionType"),
                ] {
                    if let Some(v) = code(key) {
                        *field = Some(v);
                    }
                }
                if let Some(v) = num("turnAngle") {
                    nav.turn_angle = Some(v as i32);
                }
                if let Some(v) = num("etaEpoch").filter(|v| *v > 0.0) {
                    nav.eta = Some(v);
                }
            }
            Some("nowplaying") => {
                let np = &mut entry.now_playing;
                for (field, key) in [
                    (&mut np.title, "title"),
                    (&mut np.artist, "artist"),
                    (&mut np.album, "album"),
                    (&mut np.app, "appName"),
                ] {
                    if let Some(v) = text(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in
                    [(&mut np.duration_ms, "durationMs"), (&mut np.elapsed_ms, "elapsedMs")]
                {
                    if let Some(v) = num(key) {
                        *field = Some(v);
                    }
                }
                if let Some(v) = num("playing") {
                    np.playing = Some(v == 1.0);
                }
            }
            Some("albumart") => match text("dataB64") {
                Some(b64) if !b64.is_empty() => entry.now_playing.artwork = Some(b64),
                _ => return,
            },
            _ => return,
        }
        if self.is_active(link) {
            self.publish();
        }
    }

    fn auto_activate(&mut self, index: u32) {
        if self.active().is_none() {
            self.activate(index);
        }
    }

    fn activate(&mut self, index: u32) {
        let Some(target) = self.sessions.iter().position(|s| s.index == index) else { return };
        if self.sessions[target].active {
            return;
        }
        let prev = self.sessions.iter().position(|s| s.active);
        let had_prev = prev.is_some();
        if let Some(p) = prev {
            self.sessions[p].active = false;
        }
        self.sessions[target].active = true;
        println!("[core] active session #{index}");
        self.on_active_changed(Some(target), had_prev);
        self.changed();
    }

    fn activate_next(&mut self) {
        if self.sessions.len() <= 1 {
            return;
        }
        let pos = self.sessions.iter().position(|s| s.active);
        let next = pos.map_or(0, |p| (p + 1) % self.sessions.len());
        self.activate(self.sessions[next].index);
    }

    /// The helper reads its settings at start, and a phone takes the displays
    /// it is offered only when it connects.
    fn apply_settings(&mut self) {
        println!("[core] applying the settings: helper restart, phones reconnect");
        self.apply_due = None;
        self.hub.apply();
        self.helper_restart.notify_one();
        self.cp.send(CpCmd::DropSessions);
    }

    fn apply_when_idle(&mut self) {
        self.apply_due = (self.sessions.is_empty() && self.hub.apply_pending())
            .then(|| tokio::time::Instant::now() + APPLY_AFTER_QUIET);
    }

    fn apply_idle(&mut self) {
        self.apply_due = None;
        if !self.sessions.is_empty() {
            return;
        }
        if let Some((before, after)) = self.hub.apply()
            && helper::starts_differently(&before, &after)
        {
            println!("[core] settings applied with no phone connected, the helper restarts");
            self.helper_restart.notify_one();
        }
    }

    fn activity(&self, change: impl FnOnce(&mut Activity)) {
        if let Some(activity) = &self.activity {
            activity.send_modify(change);
        }
    }

    /// `done` fires once the last session ended and the helper has the empty paging list.
    fn goodbye(&mut self, done: oneshot::Sender<()>) {
        let paging_stopped = self.devices.stop_paging();
        let (ended, sessions_ended) = oneshot::channel();
        tokio::spawn(async move {
            paging_stopped.await;
            let _ = sessions_ended.await;
            let _ = done.send(());
        });
        if self.sessions.is_empty() {
            let _ = ended.send(());
            return;
        }
        println!("[core] goodbye to {} phone(s)", self.sessions.len());
        self.bye = Some(ended);
        for s in &self.sessions {
            self.tell(s.link, Do::Close);
        }
    }

    fn close_by_link(&mut self, link: Link) {
        let Some(i) = self.sessions.iter().position(|s| s.link == link) else { return };
        let closed = self.sessions.remove(i);
        self.devices.registry.clear_presence(&registry_ids(&closed.device));
        if self.sessions.is_empty() {
            self.next_index = 1;
            if let Some(done) = self.bye.take() {
                let _ = done.send(());
            }
            self.apply_when_idle();
        }
        if closed.active {
            match self.sessions.iter().position(|s| !s.active) {
                Some(next) => {
                    self.sessions[next].active = true;
                    println!(
                        "[core] session #{} ends, #{} moves up",
                        closed.index, self.sessions[next].index
                    );
                    self.on_active_changed(Some(next), true);
                }
                None => {
                    println!("[core] session #{} ends, nothing left", closed.index);
                    self.on_active_changed(None, true);
                }
            }
        }
        self.changed();
    }

    fn close_on_transport(&mut self, ids: &Ids, transport: Transport) {
        if let Some(i) = self.by_identity(ids)
            && self.sessions[i].transport == transport
        {
            self.tell(self.sessions[i].link, Do::Close);
        }
    }

    fn on_active_changed(&mut self, next: Option<usize>, had_prev: bool) {
        self.attention = None;
        self.activity(|a| *a = Activity::default());
        match next {
            Some(i) => {
                let (level, ramp) = (self.sessions[i].duck_level, self.sessions[i].duck_ramp_ms);
                self.duck(level, ramp);
                if !had_prev {
                    self.duck_level = 1.0;
                    self.duck_ramp_ms = RAMP_UP_MS;
                }
                self.tell(self.sessions[i].link, Do::Keyframe);
            }
            None => {
                self.plane.dispose();
                self.clusters.dispose();
                self.duck_level = 1.0;
                self.duck_ramp_ms = RAMP_UP_MS;
            }
        }
    }

    fn changed(&mut self) {
        let owner = self.active().map(|s| s.link);
        if owner != self.video_owner {
            if let Some(old) = self.video_owner {
                self.tell(old, Do::VideoActive(false));
            }
            self.video_owner = owner;
            if let Some(new) = owner {
                self.tell(new, Do::VideoActive(true));
                self.push_all_levels();
            }
        }
        self.push_wired(false);
        self.publish();
    }

    fn push_wired(&mut self, again: bool) {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| matches!(s.link, Link::Aa(_)) && s.transport == Transport::Usb)
            .flat_map(|s| {
                let d = &s.device;
                [&d.instance_id, &d.bt_mac, &d.usb_serial].into_iter().flatten().cloned()
            })
            .collect();
        if let Some(io) = self.hands_free.wired(ids, again) {
            let _ = self.phone_io.send(io);
        }
    }

    fn front_main(&self) -> Front {
        if self.reverse_camera() {
            return Front::Camera;
        }
        let wants = if self.attention.is_some() { Front::Projection } else { self.wants.main };
        match wants {
            Front::Projection if self.active().is_some() => Front::Projection,
            Front::Projection => Front::Livi,
            other => other,
        }
    }

    /// Back on the projection the phone goes to its home screen, unless a
    /// call or Siri brought it there, which home would end. The plane needs a
    /// full picture either way.
    fn asked(&mut self, asked: PerScreen<Front>) {
        let before = self.wants.main;
        let last = std::mem::replace(&mut self.asked, asked);
        // While reversing, main follows the camera core put there, not a choice of the user.
        let camera = self.reverse_camera();
        for screen in [Screen::Main, Screen::Cluster, Screen::Aux] {
            if asked.get(screen) != last.get(screen) && !(camera && screen == Screen::Main) {
                *self.wants.get_mut(screen) = *asked.get(screen);
            }
        }
        if !camera && asked.main != last.main && asked.main != Front::Projection {
            self.attention = None;
        }
        let cluster_on = shows_cluster(&self.wants);
        if cluster_on != self.cluster_on {
            self.cluster_on = cluster_on;
            self.cp.send(CpCmd::ClusterActive(cluster_on));
            self.aa.send(AaCmd::ClusterActive(cluster_on));
        }
        if self.wants.main == Front::Projection && before != Front::Projection {
            if self.attention.is_none() {
                self.to_active(Do::Home);
            }
            self.keyframe();
        }
        self.publish();
    }

    fn reverse_camera(&self) -> bool {
        let cfg = &self.config_seen;
        self.reversing
            && self.camera_found
            && cfg.auto_switch_on_reverse
            && cfg.camera.main
            && !cfg.camera_id.is_empty()
    }

    fn camera_found(&mut self, found: bool) {
        let before = self.reverse_camera();
        self.camera_found = found;
        self.camera_follows(before);
    }

    fn camera_follows(&mut self, before: bool) {
        let now = self.reverse_camera();
        if now != before {
            println!("[core] reversing camera {}", if now { "in front" } else { "gone" });
            self.publish();
        }
    }

    fn attend(&mut self, kind: Attention, active: bool) {
        if active {
            if self.front_main() == Front::Projection {
                if let Some((armed, _)) = self.attention.as_mut() {
                    *armed = kind;
                }
                return;
            }
            self.attention = Some((kind, self.wants.main));
        } else {
            match self.attention {
                Some((armed, back)) if armed == kind => {
                    self.attention = None;
                    self.wants.main = back;
                }
                _ => return,
            }
        }
        self.publish();
    }

    fn host_ui(&mut self) {
        self.attention = None;
        if self.wants.main == Front::Projection {
            self.wants.main = Front::Livi;
        }
        self.publish();
    }

    /// A phone's place among the sessions and whether it is the active one.
    fn session_of(&self, ids: &devices::Ids) -> Option<(u32, bool)> {
        let ids = session_ids(ids);
        if !ids.any() {
            return None;
        }
        let i = self.sessions.iter().position(|s| s.device.overlaps(&ids))?;
        Some((i as u32 + 1, self.sessions[i].active))
    }

    fn targets(&self) -> Vec<(String, Option<String>)> {
        let on_cable = |mac: &str| {
            self.sessions.iter().any(|s| {
                s.transport == Transport::Usb
                    && s.device.bt_mac.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(mac))
            })
        };
        devices::reconnect_targets(
            &self.devices.registry,
            self.hub.config().auto_conn,
            &|ids| self.session_of(ids).is_some(),
            &on_cable,
        )
    }

    fn select_device(&mut self, id: &str) {
        let ids = match self.devices.registry.by_id(id) {
            Some(e) => e.ids(),
            None => devices::Ids {
                bt_mac: Some(id.to_string()),
                wifi_mac: Some(id.to_string()),
                usb_udid: Some(id.to_string()),
                instance_id: Some(id.to_string()),
                ..Default::default()
            },
        };
        match self.by_identity(&session_ids(&ids)) {
            Some(i) => {
                println!("[core] picked {id}, session #{}", self.sessions[i].index);
                let index = self.sessions[i].index;
                self.activate(index);
            }
            None => {
                let entry = self.devices.registry.by_id(id);
                let protocol = entry.and_then(|e| e.view_protocol());
                let mac = entry
                    .and_then(|e| e.bt_mac().map(str::to_string))
                    .or_else(|| id.contains(':').then(|| id.to_string()));
                if let Some(mac) = mac {
                    tokio::spawn(devices::wake(mac, protocol));
                }
            }
        }
    }

    fn forget_device(&mut self, id: &str) {
        let Some(gone) = self.devices.registry.forget(id) else { return };
        if let Some(i) = self.by_identity(&session_ids(&gone.ids())) {
            let session = self.sessions[i].link;
            println!("[core] forget {id} ends session #{}", self.sessions[i].index);
            let (cp, aa) = (self.cp.clone(), self.aa.clone());
            tokio::spawn(async move {
                tokio::time::sleep(FORGET_GRACE).await;
                match session {
                    Link::Cp(id) => cp.send(CpCmd::Close { id }),
                    Link::Aa(id) => aa.send(AaCmd::Close { id }),
                }
            });
        }
        if let Some(mac) = gone.bt_mac() {
            tokio::spawn(devices::unpair(mac.to_string()));
        }
        self.publish();
    }

    fn publish(&mut self) {
        let list = devices::views(&self.devices.registry, &|ids| self.session_of(ids));
        let targets = self.targets();
        self.devices.page(targets, false);
        let main = self.front_main();
        let (cluster, aux) = (self.wants.cluster, self.wants.aux);
        let position = self.sessions.iter().position(|s| s.active);
        let sessions = Sessions {
            active: position.map(|i| self.sessions[i].link.protocol()),
            position: position.map_or(0, |i| i as u32 + 1),
            total: self.sessions.len() as u32,
        };
        let (now_playing, mut navigation) = self
            .active()
            .map(|s| (s.now_playing.clone(), s.navigation.clone()))
            .unwrap_or_default();
        written_out(&mut navigation, nav_text::Language::of(&self.config_seen.language));
        self.hub.update(|s| {
            s.front.main = main;
            s.front.cluster = cluster;
            s.front.aux = aux;
            s.sessions = sessions;
            s.now_playing = now_playing;
            s.navigation = navigation;
            s.devices = list;
        });
    }

    fn keyframe(&self) {
        self.to_active(Do::Keyframe);
    }

    fn level(&self, kind: AudioKind) -> f64 {
        let duck = if kind == AudioKind::Media { self.duck_level } else { 1.0 };
        self.levels.get(kind) * duck
    }

    fn push_level(&self, kind: AudioKind, ramp_ms: u32) {
        self.to_active(Do::Volume { kind, level: self.level(kind), ramp_ms });
    }

    fn push_all_levels(&self) {
        for kind in [AudioKind::Media, AudioKind::Alert, AudioKind::Speech, AudioKind::Call] {
            self.push_level(kind, 0);
        }
    }

    fn duck(&mut self, level: f64, ramp_ms: u32) {
        self.duck_level = level.clamp(0.0, 1.0);
        self.duck_ramp_ms = ramp_ms;
        self.push_level(AudioKind::Media, ramp_ms);
    }

    fn on_state(&mut self, cfg: Config, telemetry: Map<String, Value>) {
        let camera = self.reverse_camera();
        if cfg != self.config_seen {
            self.on_config(&cfg);
            let language = cfg.language != self.config_seen.language;
            self.config_seen = cfg;
            if language {
                self.publish();
            }
            self.apply_when_idle();
        }
        if telemetry != self.telemetry_seen {
            self.cp_telemetry.follow(&self.telemetry_seen, &telemetry, &self.cp);
            self.aa_telemetry.follow(&self.telemetry_seen, &telemetry, &self.aa);
            if let Some(reversing) = reversing(&telemetry) {
                self.reversing = reversing;
            }
            self.telemetry_seen = telemetry;
        }
        self.camera_follows(camera);
    }

    fn on_config(&mut self, cfg: &Config) {
        let next = Levels::from(cfg);
        for kind in [AudioKind::Media, AudioKind::Alert, AudioKind::Speech, AudioKind::Call] {
            let (prev, to) = (self.levels.get(kind), next.get(kind).clamp(0.0, 1.0));
            if (prev - to).abs() < 0.0001 {
                continue;
            }
            match kind {
                AudioKind::Media => self.levels.music = to,
                AudioKind::Alert => self.levels.nav = to,
                AudioKind::Speech => self.levels.voice = to,
                AudioKind::Call => self.levels.call = to,
            }
            self.push_level(kind, if prev > to { RAMP_DOWN_MS } else { RAMP_UP_MS });
        }
        self.cp.send(CpCmd::InitialNightMode(night_mode(cfg.appearance_mode)));
        self.bindings = cfg.bindings.clone();
    }

    fn media(&self, control: MediaControl) {
        self.to_active(Do::Media(control));
    }

    fn seek(&self, ms: u32) {
        if let Some(s) = self.active().filter(|s| matches!(s.link, Link::Cp(_))) {
            self.devices.seek(ms, s.device.bt_mac.clone());
        }
    }

    fn input(&self, input: Input) {
        match input {
            Input::Pointer { screen: Screen::Main, points } => self.pointer(&points),
            Input::Pointer { .. } => {}
            Input::Key { code, down } => self.key(&code, down),
        }
    }

    fn pointer(&self, points: &[Point]) {
        if points.is_empty() || self.front_main() != Front::Projection {
            return;
        }
        match self.active().map(|s| s.link) {
            Some(Link::Cp(id)) => {
                let touches = points
                    .iter()
                    .map(|p| Touch {
                        x: p.x.clamp(0.0, 1.0),
                        y: p.y.clamp(0.0, 1.0),
                        down: !matches!(p.phase, Phase::Up | Phase::Cancel),
                    })
                    .collect();
                self.cp.stack(id, StackCmd::Touches(touches));
            }
            Some(Link::Aa(id)) => {
                let crop = *self.aa_crop.borrow();
                let touches = points
                    .iter()
                    .map(|p| {
                        let (x, y) = android_auto::to_stream(
                            p.x.clamp(0.0, 1.0),
                            p.y.clamp(0.0, 1.0),
                            crop.as_ref(),
                        );
                        let action = match p.phase {
                            Phase::Down => TouchAction::Down,
                            Phase::Move => TouchAction::Move,
                            Phase::Up | Phase::Cancel => TouchAction::Up,
                        };
                        TouchItem { id: p.id, x, y, action }
                    })
                    .collect();
                self.aa.session(id, SessionCmd::MultiTouch(touches));
            }
            None => {}
        }
    }

    fn key(&self, code: &str, down: bool) {
        let Some(key) = bound(&self.bindings, code) else { return };
        if self.front_main() != Front::Projection && !key.works_behind() {
            return;
        }
        match self.active().map(|s| s.link) {
            Some(Link::Cp(id)) if down => self.cp_key(id, key),
            Some(Link::Aa(id)) => self.aa_key(id, key, down),
            _ => {}
        }
    }

    /// Only the press counts, Siri on CarPlay is no push-to-talk.
    fn cp_key(&self, id: SessionId, key: Key) {
        let none = KnobState::default();
        let cmd = match key {
            Key::Left => knob(KnobState { x: -KNOB_DEFLECT, ..none }),
            Key::Right => knob(KnobState { x: KNOB_DEFLECT, ..none }),
            Key::Up => knob(KnobState { y: -KNOB_DEFLECT, ..none }),
            Key::Down => knob(KnobState { y: KNOB_DEFLECT, ..none }),
            Key::SelectDown | Key::KnobDown => StackCmd::KnobSelect(true),
            Key::SelectUp | Key::KnobUp => StackCmd::KnobSelect(false),
            Key::Back => knob(KnobState { back: true, ..none }),
            Key::Home => knob(KnobState { home: true, ..none }),
            Key::KnobLeft => knob(KnobState { wheel: -1.0, ..none }),
            Key::KnobRight => knob(KnobState { wheel: 1.0, ..none }),
            Key::PlayPause => StackCmd::Media(media_button::PLAY_PAUSE),
            Key::Play => StackCmd::Media(media_button::PLAY),
            Key::Pause => StackCmd::Media(media_button::PAUSE),
            Key::Next => StackCmd::Media(media_button::NEXT),
            Key::Prev => StackCmd::Media(media_button::PREV),
            Key::AcceptPhone => StackCmd::Telephony(telephony_button::HOOK_SWITCH),
            Key::RejectPhone => StackCmd::Telephony(telephony_button::DROP),
            Key::Phone(button) => StackCmd::Telephony(button),
            Key::VoiceAssistant => StackCmd::Siri,
            // The UI's own button for it is nextDevice.
            Key::CycleSession | Key::VoiceAssistantRelease => return,
        };
        self.cp.stack(id, cmd);
        if key == Key::SelectDown {
            let cp = self.cp.clone();
            tokio::spawn(async move {
                tokio::time::sleep(SELECT_RELEASE).await;
                cp.stack(id, StackCmd::KnobSelect(false));
            });
        }
    }

    /// Android Auto's assistant listens while the voice key is held.
    fn aa_key(&self, id: AaId, key: Key, down: bool) {
        use telephony_button as t;
        let command = match (key, down) {
            (Key::VoiceAssistant, false) => Command::VoiceAssistantRelease,
            (_, false) => return,
            (Key::Left, _) => Command::Left,
            (Key::Right, _) => Command::Right,
            (Key::Up, _) => Command::Up,
            (Key::Down, _) => Command::Down,
            (Key::SelectDown, _) => Command::SelectDown,
            (Key::SelectUp, _) => Command::SelectUp,
            (Key::KnobDown, _) => Command::KnobDown,
            (Key::KnobUp, _) => Command::KnobUp,
            (Key::Back, _) => Command::Back,
            (Key::Home, _) => Command::Home,
            (Key::KnobLeft, _) => Command::KnobLeft,
            (Key::KnobRight, _) => Command::KnobRight,
            (Key::PlayPause, _) => Command::PlayPause,
            (Key::Play, _) => Command::Play,
            (Key::Pause, _) => Command::Pause,
            (Key::Next, _) => Command::Next,
            (Key::Prev, _) => Command::Prev,
            (Key::AcceptPhone, _) => Command::AcceptPhone,
            (Key::RejectPhone, _) => Command::RejectPhone,
            (Key::VoiceAssistant, _) => Command::VoiceAssistant,
            (Key::VoiceAssistantRelease, _) => Command::VoiceAssistantRelease,
            (Key::Phone(b), _) if b == t::STAR => Command::PhoneKeyStar,
            (Key::Phone(b), _) if b == t::POUND => Command::PhoneKeyHash,
            (Key::Phone(b), _) if b == t::HOOK_SWITCH => Command::PhoneKeyHookSwitch,
            (Key::Phone(b), _) => match b.wrapping_sub(t::KEY0) {
                0 => Command::PhoneKey0,
                1 => Command::PhoneKey1,
                2 => Command::PhoneKey2,
                3 => Command::PhoneKey3,
                4 => Command::PhoneKey4,
                5 => Command::PhoneKey5,
                6 => Command::PhoneKey6,
                7 => Command::PhoneKey7,
                8 => Command::PhoneKey8,
                9 => Command::PhoneKey9,
                _ => return,
            },
            (Key::CycleSession, _) => return,
        };
        self.aa.session(id, SessionCmd::Command(command));
        if key == Key::SelectDown {
            let aa = self.aa.clone();
            tokio::spawn(async move {
                tokio::time::sleep(SELECT_RELEASE).await;
                aa.session(id, SessionCmd::Command(Command::SelectUp));
            });
        }
    }

    fn on_duck(&mut self, link: Link, level: f64, duration_ms: u32) {
        let Some(s) = self.by_link(link) else { return };
        s.duck_level = level;
        s.duck_ramp_ms = duration_ms;
        if self.is_active(link) {
            self.duck(if level >= 1.0 { 1.0 } else { level }, duration_ms);
        }
    }

    fn on_aa(&mut self, ev: AaEvent) {
        match ev {
            AaEvent::Spawned { id, wired, usb_serial, .. } => {
                self.aa_spawned.insert(id, (wired, usb_serial));
            }
            AaEvent::Connected { id } => {
                let (wired, usb_serial) =
                    self.aa_spawned.get(&id).cloned().unwrap_or((false, None));
                println!(
                    "[core] Android Auto projects over {}",
                    if wired { "usb" } else { "wifi" }
                );
                let ids =
                    Ids { usb_serial: usb_serial.filter(|s| !s.is_empty()), ..Default::default() };
                let index = self.upsert(Link::Aa(id), transport(wired), &ids);
                self.auto_activate(index);
                self.ensure_aa_links();
                self.aa_telemetry.hydrate(&self.telemetry_seen, &self.aa);
            }
            AaEvent::HelperConnected => self.push_wired(true),
            AaEvent::Helper(ev) => self.on_aa_helper(&ev),
            AaEvent::Device { id, name, model, instance_id, ip } => {
                self.on_aa_device(id, name, model, instance_id, ip);
            }
            AaEvent::Status { id, status } => {
                let link = Link::Aa(id);
                let ids = match self.sessions.iter().find(|s| s.link == link) {
                    Some(s) => registry_ids(&s.device),
                    // The battery can come before the session has its phone, the address still names it.
                    None => devices::Ids { ip: opt(&status.ip), ..Default::default() },
                };
                if status.battery_level.is_some() {
                    self.hands_free.battery_precise = true;
                }
                let status = devices::Status {
                    battery_level: status.battery_level.map(f64::from),
                    signal_strength: status.signal_strength.map(f64::from),
                    ..Default::default()
                };
                self.devices.registry.note_status(&ids, &status);
                self.publish();
            }
            AaEvent::Disconnected { id } => {
                self.aa_spawned.remove(&id);
                self.close_by_link(Link::Aa(id));
            }
            AaEvent::Audio { id, stream, active, .. } => {
                // A stream opens at the host's default, it takes its level once it plays.
                if active && self.is_active(Link::Aa(id)) {
                    self.push_level(cp_kind(AaKind::of(stream)), 0);
                }
                if self.is_active(Link::Aa(id)) {
                    self.activity(|a| a.stream(cp_kind(AaKind::of(stream)), active));
                }
            }
            AaEvent::Duck { id, level, duration_ms } => {
                self.on_duck(Link::Aa(id), level, duration_ms);
            }
            AaEvent::HostUiRequested { id } => {
                println!("[core] Android Auto asks for the car's own UI");
                if self.is_active(Link::Aa(id)) {
                    self.host_ui();
                }
            }
            AaEvent::NowPlaying { id, now_playing } => self.aa_now_playing(id, now_playing),
            AaEvent::Navigation { id, navigation } => self.aa_navigation(id, navigation),
            AaEvent::NavigationImage { id, image } => {
                let link = Link::Aa(id);
                let Some(entry) = self.by_link(link) else { return };
                entry.navigation.image = (!image.is_empty()).then(|| STANDARD.encode(&image));
                if self.is_active(link) {
                    self.publish();
                }
            }
            _ => {}
        }
    }

    fn on_aa_helper(&mut self, ev: &Value) {
        let text = |key: &str| ev.get(key).and_then(Value::as_str).filter(|v| !v.is_empty());
        let up = ev.get("up").and_then(Value::as_bool) == Some(true);
        match text("event") {
            Some("sco") => self.call_audio(up),
            Some("hfp") => self.hands_free.on_link(up, text("mac")),
            Some("phone-battery") => {
                let pct = ev.get("pct").and_then(Value::as_f64);
                let Some(mac) = text("mac") else { return };
                println!("[core] HFP battery {mac}: ~{}%", pct.unwrap_or(0.0));
                // Steps of 20 %, only until Android Auto's own reading arrives.
                if self.hands_free.battery_precise || pct.is_none() {
                    return;
                }
                let ids = devices::Ids { bt_mac: Some(mac.to_string()), ..Default::default() };
                let status = devices::Status { battery_level: pct, ..Default::default() };
                self.devices.registry.note_status(&ids, &status);
                self.publish();
            }
            Some("aa-device") => {
                let Some(instance) = text("instanceId") else { return };
                if let Some(mac) = text("btMac") {
                    self.hands_free.bt_by_instance.insert(instance.into(), mac.into());
                    self.keep_link(mac);
                }
                if let Some(serial) = text("usbSerial") {
                    self.hands_free.serial_by_instance.insert(instance.into(), serial.into());
                }
            }
            _ => {}
        }
    }

    fn call_audio(&mut self, up: bool) {
        if self.hands_free.call == up {
            return;
        }
        self.hands_free.call = up;
        self.activity(|a| a.phone = up);
        let _ = self.phone_io.send(Io::Call(up));
        self.attend(Attention::Call, up);
    }

    fn on_aa_device(
        &mut self,
        id: AaId,
        name: String,
        model: String,
        instance: String,
        ip: String,
    ) {
        let link = Link::Aa(id);
        let wired = match self.sessions.iter().find(|s| s.link == link) {
            Some(s) => s.transport == Transport::Usb,
            None => self.aa_spawned.get(&id).is_some_and(|(wired, _)| *wired),
        };
        let instance_id = opt(&instance);
        let lookup =
            |by: &HashMap<String, String>| instance_id.as_ref().and_then(|i| by.get(i)).cloned();
        let bt_mac = if wired { None } else { lookup(&self.hands_free.bt_by_instance) };
        let usb_serial = self
            .aa_spawned
            .get(&id)
            .and_then(|(_, serial)| serial.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| lookup(&self.hands_free.serial_by_instance));
        let ids = Ids { bt_mac, usb_serial, instance_id, ip: opt(&ip), ..Default::default() };
        let seen = Seen {
            ids: registry_ids(&ids),
            name: opt(&name),
            model: opt(&model),
            protocol: Some(Protocol::Androidauto),
            wired,
        };
        self.devices.registry.note_device(&seen);
        let index = self.upsert(link, transport(wired), &ids);
        self.auto_activate(index);
    }

    /// Wireless calls need the phone's hands-free link.
    fn ensure_aa_links(&mut self) {
        let known: Vec<String> = self.hands_free.bt_by_instance.values().cloned().collect();
        if known.is_empty() {
            let _ = self.phone_io.send(Io::FindPhones);
            return;
        }
        for mac in known {
            self.keep_link(&mac);
        }
    }

    fn on_back(&mut self, back: Back) {
        let Back::Phones(phones) = back;
        if let [mac] = phones.as_slice() {
            let unnamed: Vec<Option<String>> = self
                .sessions
                .iter()
                .filter(|s| matches!(s.link, Link::Aa(_)) && s.device.bt_mac.is_none())
                .map(|s| s.device.instance_id.clone())
                .collect();
            for instance in unnamed {
                if let Some(instance) = &instance {
                    self.hands_free.bt_by_instance.insert(instance.clone(), mac.clone());
                }
                let ids = devices::Ids {
                    bt_mac: Some(mac.clone()),
                    instance_id: instance,
                    ..Default::default()
                };
                let seen =
                    Seen { ids, protocol: Some(Protocol::Androidauto), ..Default::default() };
                self.devices.registry.note_device(&seen);
            }
            self.publish();
        }
        for mac in phones {
            self.keep_link(&mac);
        }
    }

    fn keep_link(&mut self, mac: &str) {
        if self.hands_free.keep(mac) {
            self.check_link(&mac.to_lowercase());
        }
    }

    fn keep_links(&mut self) {
        for mac in self.hands_free.kept() {
            self.check_link(&mac);
        }
    }

    fn check_link(&mut self, mac: &str) {
        let wanted = self.hands_free.bt_by_instance.values().any(|m| m.eq_ignore_ascii_case(mac))
            || self.sessions.iter().any(|s| matches!(s.link, Link::Aa(_)));
        if let Some(io) = self.hands_free.check(mac, wanted, tokio::time::Instant::now()) {
            let _ = self.phone_io.send(io);
        }
    }

    fn aa_now_playing(&mut self, id: AaId, from: AaNowPlaying) {
        let link = Link::Aa(id);
        let Some(entry) = self.by_link(link) else { return };
        let np = &mut entry.now_playing;
        for (field, value) in [
            (&mut np.title, from.title),
            (&mut np.artist, from.artist),
            (&mut np.album, from.album),
            (&mut np.app, from.app),
        ] {
            if value.is_some() {
                *field = value;
            }
        }
        if let Some(ms) = from.duration_ms {
            np.duration_ms = Some(ms as f64);
        }
        if let Some(ms) = from.elapsed_ms {
            np.elapsed_ms = Some(ms as f64);
        }
        if from.playing.is_some() {
            np.playing = from.playing;
        }
        if let Some(art) = from.artwork.filter(|a| !a.is_empty()) {
            np.artwork = Some(STANDARD.encode(art));
        }
        if self.is_active(link) {
            self.publish();
        }
    }

    fn aa_navigation(&mut self, id: AaId, from: AaNavigation) {
        let link = Link::Aa(id);
        let Some(entry) = self.by_link(link) else { return };
        let image = entry.navigation.image.take();
        entry.navigation = if from.active == Some(false) {
            Navigation { active: Some(false), ..Default::default() }
        } else {
            Navigation {
                active: from.active,
                road_name: from.road_name,
                destination_name: from.destination_name,
                time_to_destination: from.time_to_destination.map(f64::from),
                distance_to_destination: from.distance_to_destination.map(f64::from),
                remain_distance: from.remain_distance.map(f64::from),
                maneuver_type: from.maneuver_type.map(|m| m as u32),
                turn_side: from.turn_side.map(|s| s as u32),
                turn_angle: from.turn_angle.and_then(|a| i32::try_from(a).ok()),
                eta_text: from.eta,
                app_name: from.app_name,
                image,
                ..Default::default()
            }
        };
        if self.is_active(link) {
            self.publish();
        }
    }
}

fn written_out(nav: &mut Navigation, language: nav_text::Language) {
    nav.maneuver_text = nav.maneuver_type.and_then(|code| nav_text::maneuver(code, language));
    nav.maneuver_distance_text = nav.remain_distance.and_then(nav_text::distance);
    nav.destination_distance_text = nav.distance_to_destination.and_then(nav_text::distance);
    nav.time_left_text = nav.time_to_destination.and_then(nav_text::time_left);
}

async fn player_created(
    rx: &mut Option<broadcast::Receiver<()>>,
) -> Result<(), broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn transport(wired: bool) -> Transport {
    if wired { Transport::Usb } else { Transport::Wifi }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::config::{AppearanceMode, defaults};
    use livi_core_proto::input::Point;
    use livi_cp::stack::AudioProfile;
    use livi_media::compositor::CompositorControl;
    use livi_media::gst_host::GstHost;
    use serde_json::json;

    use super::*;
    use crate::config_file::tests::TempDir;

    struct Rig {
        dir: TempDir,
        hub: Arc<Hub>,
        p: Projection,
        cmds: mpsc::UnboundedReceiver<CpCmd>,
        aa: mpsc::UnboundedReceiver<AaCmd>,
        crop: watch::Sender<Option<Crop>>,
        io: mpsc::UnboundedReceiver<Io>,
        telemetry: mpsc::UnboundedReceiver<Value>,
    }

    fn livi() -> PerScreen<Front> {
        PerScreen { main: Front::Livi, cluster: Front::Livi, aux: Front::Livi }
    }

    impl Rig {
        fn new() -> Self {
            let dir = TempDir::new();
            let hub = Arc::new(Hub::new(State {
                front: livi(),
                sessions: Default::default(),
                now_playing: Default::default(),
                telemetry: Default::default(),
                navigation: Default::default(),
                system: Default::default(),
                devices: Default::default(),
                update: Default::default(),
                config: defaults(),
            }));
            let (tx, cmds) = mpsc::unbounded_channel();
            let (aa_tx, mut aa) = mpsc::unbounded_channel();
            let (crop, aa_crop) = watch::channel(None);
            let gst = GstHost::new(dir.0.join("gst.sock"));
            let ctrl = CompositorControl::connect(dir.0.join("ctrl"));
            let plane = MainPlane::new(gst.clone(), ctrl.clone());
            let clusters = ClusterPlanes::new(gst, ctrl);
            let devices = Devices::new(
                &dir.0.join("devices.json"),
                livi_cp::helper_sock::HelperSock::new(dir.0.join("cp-bt.sock")),
            );
            let (phone_io, io) = mpsc::unbounded_channel();
            let (telemetry_tx, telemetry) = mpsc::unbounded_channel();
            let drivers = Drivers {
                cp: CpHandle::from_sender(tx),
                aa: AaHandle::from_sender(aa_tx),
                aa_crop,
                phone_io,
                phone_back: mpsc::unbounded_channel().1,
                telemetry: telemetry_tx,
            };
            let p = Projection::new(
                hub.clone(),
                drivers,
                plane,
                clusters,
                Arc::new(Notify::new()),
                devices,
                livi(),
            );
            assert_eq!(aa.try_recv(), Ok(AaCmd::ClusterActive(false)));
            let mut rig = Self { dir, hub, p, cmds, aa, crop, io, telemetry };
            assert_eq!(rig.drain(), [CpCmd::InitialNightMode(None), CpCmd::ClusterActive(false)]);
            rig
        }

        fn io_drain(&mut self) -> Vec<Io> {
            let mut out = Vec::new();
            while let Ok(io) = self.io.try_recv() {
                out.push(io);
            }
            out
        }

        fn aa_drain(&mut self) -> Vec<AaCmd> {
            let mut out = Vec::new();
            while let Ok(cmd) = self.aa.try_recv() {
                out.push(cmd);
            }
            out
        }

        fn drain(&mut self) -> Vec<CpCmd> {
            let mut out = Vec::new();
            while let Ok(cmd) = self.cmds.try_recv() {
                out.push(cmd);
            }
            out
        }

        fn connect(&mut self, id: SessionId, wired: bool) {
            self.p.on_event(CpEvent::Connected { id, wired, controller_id: None });
        }

        fn device(&mut self, id: SessionId, wired: bool, bt: &str, wifi: &str) {
            self.p.on_event(CpEvent::Presence {
                id,
                wired,
                presence: Presence::Device {
                    bt_mac: bt.into(),
                    wifi_mac: wifi.into(),
                    ip: String::new(),
                    usb_udid: String::new(),
                    name: String::new(),
                    model: String::new(),
                },
            });
        }

        fn want(&mut self, front: Front) {
            self.p.asked(PerScreen { main: front, ..livi() });
        }

        fn front(&self) -> Front {
            self.hub.watch().borrow().front.main
        }

        fn now_playing(&self) -> NowPlaying {
            self.hub.watch().borrow().now_playing.clone()
        }

        fn navigation(&self) -> Navigation {
            self.hub.watch().borrow().navigation.clone()
        }

        fn meta(&mut self, id: SessionId, event: Value) {
            self.p.on_event(CpEvent::Metadata { id, event });
        }

        fn sessions(&self) -> (Option<Protocol>, u32, u32) {
            let s = self.hub.watch().borrow().sessions;
            (s.active, s.position, s.total)
        }

        fn settings(&mut self, change: impl FnOnce(&mut Config)) {
            self.hub.update(|s| change(&mut s.config));
            let (cfg, telemetry) = {
                let s = self.hub.watch();
                let s = s.borrow();
                (s.config.clone(), s.telemetry.clone())
            };
            self.p.on_state(cfg, telemetry);
        }
    }

    fn stack(id: SessionId, cmd: StackCmd) -> CpCmd {
        CpCmd::Stack { id, cmd }
    }

    fn volume(id: SessionId, kind: AudioKind, level: f64, ramp_ms: u32) -> CpCmd {
        stack(id, StackCmd::StreamVolume { kind, level, ramp_ms })
    }

    fn home() -> StackCmd {
        knob(KnobState { home: true, ..Default::default() })
    }

    fn press(code: &str) -> Input {
        Input::Key { code: code.into(), down: true }
    }

    #[tokio::test]
    async fn the_first_phone_projects_where_the_ui_asks() {
        let mut rig = Rig::new();
        rig.want(Front::Projection);
        assert_eq!(rig.front(), Front::Livi);
        rig.connect(1, true);
        assert_eq!(rig.front(), Front::Projection);
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
        assert_eq!(
            rig.drain(),
            [
                volume(1, AudioKind::Media, 0.95, RAMP_UP_MS),
                stack(1, StackCmd::Keyframe),
                stack(1, StackCmd::VideoActive(true)),
                stack(1, StackCmd::AudioActive(true)),
                volume(1, AudioKind::Media, 0.95, 0),
                volume(1, AudioKind::Alert, 0.95, 0),
                volume(1, AudioKind::Speech, 0.95, 0),
                volume(1, AudioKind::Call, 0.95, 0),
            ]
        );
        rig.want(Front::Camera);
        assert_eq!(rig.front(), Front::Camera);
        rig.want(Front::Projection);
        rig.want(Front::Projection);
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert_eq!(rig.front(), Front::Livi);
        assert_eq!(rig.sessions(), (None, 0, 0));
        assert_eq!(
            rig.drain(),
            [
                stack(1, home()),
                stack(1, StackCmd::Keyframe),
                stack(1, StackCmd::VideoActive(false)),
                stack(1, StackCmd::AudioActive(false)),
            ]
        );
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert!(rig.p.sessions.is_empty());
    }

    #[tokio::test]
    async fn a_held_phone_moves_up() {
        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.device(2, false, "AA:BB", "cc:dd");
        rig.drain();
        assert!(rig.p.sessions[0].active && !rig.p.sessions[1].active);
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 2));
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert!(rig.p.sessions[0].active);
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
        let cmds = rig.drain();
        assert_eq!(cmds[1], stack(2, StackCmd::Keyframe));
        assert!(cmds.contains(&stack(2, StackCmd::VideoActive(true))));
        rig.p.on_event(CpEvent::Presence {
            id: 2,
            wired: false,
            presence: Presence::Active { ip: "x".into() },
        });
        rig.p.on_event(CpEvent::Presence {
            id: 2,
            wired: false,
            presence: Presence::Status {
                battery_level: None,
                battery_charging: None,
                signal_strength: None,
                carrier_name: None,
            },
        });
        assert!(rig.drain().is_empty());
    }

    #[tokio::test]
    async fn the_connection_takes_over_its_placeholder_and_the_cable_takes_over_wireless() {
        let mut rig = Rig::new();
        rig.device(5, false, "AA:BB", "");
        rig.drain();
        rig.device(6, false, "aa:bb", "11:22");
        assert_eq!(rig.p.sessions.len(), 1);
        assert_eq!(rig.p.sessions[0].link, Link::Cp(6));
        assert!(rig.drain().contains(&CpCmd::Close { id: 5 }));

        rig.device(7, true, "AA:BB", "33:44");
        assert_eq!(rig.p.sessions.len(), 1);
        assert_eq!(rig.p.sessions[0].link, Link::Cp(7));
        assert_eq!(rig.p.sessions[0].transport, Transport::Usb);
        assert_eq!(rig.p.sessions[0].device.wifi_mac.as_deref(), Some("11:22"));
        assert!(rig.drain().contains(&CpCmd::Close { id: 6 }));

        rig.device(8, false, "AA:BB", "");
        assert_eq!(rig.p.sessions.len(), 1);
        assert_eq!(rig.p.sessions[0].link, Link::Cp(8));
        assert!(rig.drain().contains(&CpCmd::Close { id: 7 }));
        let pos = rig.p.by_identity(&Ids { usb_udid: Some("u".into()), ..Default::default() });
        assert_eq!(pos, None);
    }

    #[tokio::test]
    async fn levels_follow_the_config_and_the_ducking() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.drain();
        rig.p.on_event(CpEvent::Duck { id: 1, level: 0.5, duration_ms: 300 });
        assert_eq!(rig.drain(), [volume(1, AudioKind::Media, 0.475, 300)]);
        rig.p.on_event(CpEvent::Duck { id: 1, level: 1.0, duration_ms: 100 });
        assert_eq!(rig.drain(), [volume(1, AudioKind::Media, 0.95, 100)]);
        rig.p.on_event(CpEvent::Duck { id: 9, level: 0.1, duration_ms: 0 });
        assert!(rig.drain().is_empty());

        let mut cfg = defaults();
        cfg.nav_volume = 0.5;
        cfg.call_volume = 1.0;
        cfg.appearance_mode = AppearanceMode::Night;
        rig.p.on_config(&cfg);
        assert_eq!(
            rig.drain(),
            [
                volume(1, AudioKind::Alert, 0.5, RAMP_DOWN_MS),
                volume(1, AudioKind::Call, 1.0, RAMP_UP_MS),
                CpCmd::InitialNightMode(Some(true)),
            ]
        );
        let media = AudioProfile { kind: AudioKind::Call, label: "telephony".into() };
        rig.p.on_event(CpEvent::AudioActive { id: 1, profile: media.clone(), active: true });
        assert_eq!(rig.drain(), [volume(1, AudioKind::Call, 1.0, 0)]);
        rig.p.on_event(CpEvent::AudioActive { id: 1, profile: media, active: false });
        assert!(rig.drain().is_empty());
        assert_eq!(night_mode(AppearanceMode::Day), Some(false));
    }

    #[tokio::test]
    async fn buttons_and_touch_go_to_the_active_phone() {
        let mut rig = Rig::new();
        rig.p.media(MediaControl::Next);
        assert!(rig.drain().is_empty());
        rig.connect(1, true);
        rig.drain();
        for (control, index) in [
            (MediaControl::Play, media_button::PLAY),
            (MediaControl::Pause, media_button::PAUSE),
            (MediaControl::PlayPause, media_button::PLAY_PAUSE),
            (MediaControl::Next, media_button::NEXT),
            (MediaControl::Prev, media_button::PREV),
        ] {
            rig.p.media(control);
            assert_eq!(rig.drain(), [stack(1, StackCmd::Media(index))]);
        }
        let touch = Input::Pointer {
            screen: Screen::Main,
            points: vec![
                Point { id: 0, x: 0.25, y: 1.5, phase: Phase::Down },
                Point { id: 1, x: 0.5, y: 0.5, phase: Phase::Up },
            ],
        };
        rig.p.input(touch.clone());
        assert!(rig.drain().is_empty());
        rig.want(Front::Projection);
        rig.p.input(touch);
        assert_eq!(
            rig.drain(),
            [
                stack(1, home()),
                stack(1, StackCmd::Keyframe),
                stack(
                    1,
                    StackCmd::Touches(vec![
                        Touch { x: 0.25, y: 1.0, down: true },
                        Touch { x: 0.5, y: 0.5, down: false },
                    ])
                )
            ]
        );
        rig.p.input(Input::Pointer { screen: Screen::Main, points: vec![] });
        let dash = Point { id: 0, x: 0.5, y: 0.5, phase: Phase::Down };
        rig.p.input(Input::Pointer { screen: Screen::Cluster, points: vec![dash] });
        rig.p.keyframe();
        assert_eq!(rig.drain(), [stack(1, StackCmd::Keyframe)]);
    }

    #[tokio::test]
    async fn keys_follow_the_bindings() {
        let mut rig = Rig::new();
        rig.p.input(press("KeyN"));
        assert!(rig.drain().is_empty());
        rig.connect(1, true);
        rig.drain();

        for code in ["ArrowLeft", "Digit1", "KeyN", "KeyA", "KeyV", "KeyH", "KeyQ"] {
            rig.p.input(press(code));
        }
        rig.p.input(Input::Key { code: "KeyV".into(), down: false });
        assert_eq!(
            rig.drain(),
            [
                stack(1, StackCmd::Media(media_button::NEXT)),
                stack(1, StackCmd::Telephony(telephony_button::HOOK_SWITCH)),
                stack(1, StackCmd::Siri),
                stack(1, home()),
            ]
        );

        rig.want(Front::Projection);
        rig.drain();
        let none = KnobState::default();
        let mut cfg = rig.hub.config();
        let b = &mut cfg.bindings;
        for (field, code) in [
            (&mut b.knob_left, "KeyJ"),
            (&mut b.knob_right, "KeyK"),
            (&mut b.knob_up, "KeyU"),
            (&mut b.knob_down, "KeyI"),
            (&mut b.select_up, "KeyE"),
            (&mut b.play, "KeyX"),
            (&mut b.pause, "KeyY"),
            (&mut b.phone_key_star, "KeyT"),
            (&mut b.phone_key_hash, "KeyG"),
            (&mut b.phone_key_hook_switch, "KeyO"),
            (&mut b.voice_assistant_release, "KeyW"),
        ] {
            *field = code.into();
        }
        rig.p.on_config(&cfg);
        rig.drain();
        for (code, cmd) in [
            ("ArrowLeft", knob(KnobState { x: -KNOB_DEFLECT, ..none })),
            ("ArrowRight", knob(KnobState { x: KNOB_DEFLECT, ..none })),
            ("ArrowUp", knob(KnobState { y: -KNOB_DEFLECT, ..none })),
            ("ArrowDown", knob(KnobState { y: KNOB_DEFLECT, ..none })),
            ("Backspace", knob(KnobState { back: true, ..none })),
            ("KeyH", home()),
            ("KeyJ", knob(KnobState { wheel: -1.0, ..none })),
            ("KeyK", knob(KnobState { wheel: 1.0, ..none })),
            ("KeyU", StackCmd::KnobSelect(false)),
            ("KeyI", StackCmd::KnobSelect(true)),
            ("KeyE", StackCmd::KnobSelect(false)),
            ("KeyP", StackCmd::Media(media_button::PLAY_PAUSE)),
            ("KeyX", StackCmd::Media(media_button::PLAY)),
            ("KeyY", StackCmd::Media(media_button::PAUSE)),
            ("KeyB", StackCmd::Media(media_button::PREV)),
            ("KeyR", StackCmd::Telephony(telephony_button::DROP)),
            ("Digit0", StackCmd::Telephony(telephony_button::KEY0)),
            ("Digit9", StackCmd::Telephony(telephony_button::KEY0 + 9)),
            ("KeyT", StackCmd::Telephony(telephony_button::STAR)),
            ("KeyG", StackCmd::Telephony(telephony_button::POUND)),
            ("KeyO", StackCmd::Telephony(telephony_button::HOOK_SWITCH)),
        ] {
            rig.p.input(press(code));
            assert_eq!(rig.drain(), [stack(1, cmd)], "{code}");
        }
        rig.p.input(press("KeyS"));
        rig.p.input(press("KeyW"));
        assert!(rig.drain().is_empty());
    }

    #[tokio::test]
    async fn a_select_key_clicks() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.want(Front::Projection);
        rig.drain();
        rig.p.input(press("Enter"));
        assert_eq!(rig.drain(), [stack(1, StackCmd::KnobSelect(true))]);
        tokio::time::sleep(SELECT_RELEASE * 2).await;
        assert_eq!(rig.drain(), [stack(1, StackCmd::KnobSelect(false))]);
    }

    #[tokio::test]
    async fn next_device_goes_round_the_phones() {
        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.p.activate_next();
        assert!(rig.p.sessions[0].active);
        rig.device(2, false, "AA:BB", "cc:dd");
        rig.p.activate_next();
        assert!(!rig.p.sessions[0].active && rig.p.sessions[1].active);
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 2, 2));
        rig.p.activate_next();
        assert!(rig.p.sessions[0].active);
    }

    #[tokio::test]
    async fn the_helper_ends_sessions_whose_transport_went() {
        let mut rig = Rig::new();
        rig.device(1, false, "AA:BB", "cc:dd");
        rig.device(2, true, "EE:FF", "");
        rig.p.on_event(CpEvent::Presence {
            id: 2,
            wired: true,
            presence: Presence::Device {
                bt_mac: String::new(),
                wifi_mac: String::new(),
                ip: String::new(),
                usb_udid: "u2".into(),
                name: String::new(),
                model: String::new(),
            },
        });
        rig.drain();
        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::Wifi {
            wifi_mac: "CC:DD".into(),
            ip: String::new(),
            connected: false,
        }));
        assert_eq!(rig.drain(), [CpCmd::Close { id: 1 }]);
        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::DeviceGone {
            usb_udid: "u2".into(),
        }));
        assert_eq!(rig.drain(), [CpCmd::Close { id: 2 }]);
        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::DeviceGone {
            usb_udid: "zz".into(),
        }));
        rig.p.on_event(CpEvent::HelperConnected);
        assert!(rig.drain().is_empty());
    }

    #[tokio::test]
    async fn a_call_or_siri_brings_the_projection_forward_and_back() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.drain();
        assert_eq!(rig.front(), Front::Livi);

        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ringing });
        assert_eq!(rig.front(), Front::Projection);
        rig.p.asked(PerScreen { cluster: Front::Cluster, ..livi() });
        assert_eq!(rig.front(), Front::Projection);
        // The UI follows without a home press, that would end the call or Siri.
        rig.p.asked(PerScreen {
            main: Front::Projection,
            cluster: Front::Cluster,
            aux: Front::Livi,
        });
        assert_eq!(rig.drain(), [CpCmd::ClusterActive(true), stack(1, StackCmd::Keyframe)]);

        rig.p.on_event(CpEvent::SpeechActive { id: 1, active: true });
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ended });
        assert_eq!(rig.front(), Front::Projection);
        rig.p.on_event(CpEvent::SpeechActive { id: 1, active: false });
        assert_eq!(rig.front(), Front::Livi);
        rig.want(Front::Livi);
        assert_eq!(rig.front(), Front::Livi);

        let call = AudioProfile { kind: AudioKind::Call, label: "telephony".into() };
        rig.p.on_event(CpEvent::AudioActive { id: 1, profile: call.clone(), active: true });
        assert_eq!(rig.front(), Front::Projection);
        rig.want(Front::Projection);
        rig.want(Front::Livi);
        assert_eq!(rig.front(), Front::Livi);
        rig.p.on_event(CpEvent::AudioActive { id: 1, profile: call, active: false });
        assert_eq!(rig.front(), Front::Livi);

        rig.want(Front::Projection);
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Active });
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ended });
        assert_eq!(rig.front(), Front::Projection);
    }

    #[tokio::test]
    async fn only_the_phone_in_front_moves_the_screen() {
        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.device(2, false, "AA:BB", "cc:dd");
        rig.p.on_event(CpEvent::Call { id: 2, phase: CallPhase::Ringing });
        rig.p.on_event(CpEvent::SpeechActive { id: 2, active: true });
        assert_eq!(rig.front(), Front::Livi);
        rig.want(Front::Projection);
        rig.p.on_event(CpEvent::HostUiRequested { id: 2 });
        assert_eq!(rig.front(), Front::Projection);

        rig.want(Front::Livi);
        rig.p.on_event(CpEvent::SpeechActive { id: 1, active: true });
        assert_eq!(rig.front(), Front::Projection);
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert_eq!(rig.front(), Front::Livi);
    }

    #[tokio::test]
    async fn the_phones_climate_lands_in_the_telemetry() {
        let mut rig = Rig::new();
        let climate = json!({ "auto": false }).as_object().cloned().unwrap();
        rig.p.on_event(CpEvent::Climate { id: 1, climate });
        assert_eq!(
            rig.telemetry.try_recv(),
            Ok(json!({ "climate": { "auto": false, "source": "carplay" } }))
        );
    }

    #[tokio::test]
    async fn now_playing_follows_the_phone_in_front() {
        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.device(2, false, "AA:BB", "cc:dd");
        rig.meta(
            1,
            json!({ "type": "nowplaying", "title": "One", "artist": "A", "appName": "Music",
                    "durationMs": 1000, "elapsedMs": 10, "playing": 1 }),
        );
        rig.meta(1, json!({ "type": "nowplaying", "elapsedMs": 20, "playing": 0, "album": 5 }));
        rig.meta(1, json!({ "type": "albumart", "dataB64": "AAAA" }));
        rig.meta(1, json!({ "type": "albumart", "dataB64": "" }));
        rig.meta(2, json!({ "type": "nowplaying", "title": "Two" }));
        rig.meta(9, json!({ "type": "nowplaying", "title": "Nobody" }));
        assert_eq!(
            rig.now_playing(),
            NowPlaying {
                title: Some("One".into()),
                artist: Some("A".into()),
                album: None,
                app: Some("Music".into()),
                duration_ms: Some(1000.0),
                elapsed_ms: Some(20.0),
                playing: Some(false),
                artwork: Some("AAAA".into()),
            }
        );

        rig.p.activate_next();
        assert_eq!(rig.now_playing().title.as_deref(), Some("Two"));
        rig.p.on_event(CpEvent::Disconnected { id: 2 });
        assert_eq!(rig.now_playing().title.as_deref(), Some("One"));
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert_eq!(rig.now_playing(), NowPlaying::default());
    }

    #[tokio::test]
    async fn navigation_follows_the_phone_in_front_and_clears_at_the_end() {
        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.device(2, false, "AA:BB", "cc:dd");
        rig.meta(
            1,
            json!({ "type": "navigation", "status": 1, "orderType": 2, "roadName": "Main St",
                    "afterRoadName": "Side St", "destinationName": "Home",
                    "timeToDestination": 600, "distanceToDestination": 5000,
                    "remainDistance": 200, "maneuverType": 1, "turnSide": 0,
                    "junctionType": 1, "turnAngle": -90, "etaEpoch": 1_800_000_000 }),
        );
        rig.meta(1, json!({ "type": "navigation", "remainDistance": 150, "etaEpoch": 0 }));
        rig.meta(2, json!({ "type": "navigation", "status": 1, "roadName": "Elsewhere" }));
        assert_eq!(
            rig.navigation(),
            Navigation {
                active: Some(true),
                order_type: Some(2),
                road_name: Some("Main St".into()),
                after_road_name: Some("Side St".into()),
                destination_name: Some("Home".into()),
                time_to_destination: Some(600.0),
                distance_to_destination: Some(5000.0),
                remain_distance: Some(150.0),
                maneuver_type: Some(1),
                turn_side: Some(0),
                junction_type: Some(1),
                turn_angle: Some(-90),
                eta: Some(1_800_000_000.0),
                maneuver_text: Some("Turn left".into()),
                maneuver_distance_text: Some("150 m".into()),
                destination_distance_text: Some("5.0 km".into()),
                time_left_text: Some("10 min".into()),
                ..Default::default()
            }
        );
        let mut cfg = rig.hub.config();
        cfg.language = "de".into();
        rig.p.on_state(cfg, Map::new());
        assert_eq!(rig.navigation().maneuver_text.as_deref(), Some("Links abbiegen"));

        rig.meta(1, json!({ "type": "navigation", "status": 0, "roadName": "Last" }));
        assert_eq!(
            rig.navigation(),
            Navigation {
                active: Some(false),
                road_name: Some("Last".into()),
                ..Default::default()
            }
        );
        rig.p.activate_next();
        assert_eq!(rig.navigation().road_name.as_deref(), Some("Elsewhere"));
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        rig.p.on_event(CpEvent::Disconnected { id: 2 });
        assert_eq!(rig.navigation(), Navigation::default());
    }

    #[tokio::test]
    async fn the_cluster_streams_while_a_screen_shows_it() {
        let mut rig = Rig::new();
        rig.p.asked(PerScreen { cluster: Front::Cluster, ..livi() });
        rig.p.asked(PerScreen { main: Front::Cluster, cluster: Front::Cluster, aux: Front::Livi });
        assert_eq!(rig.drain(), [CpCmd::ClusterActive(true)]);
        assert_eq!(rig.hub.watch().borrow().front.cluster, Front::Cluster);
        rig.p.asked(PerScreen { cluster: Front::Cluster, ..livi() });
        rig.p.asked(livi());
        assert_eq!(rig.drain(), [CpCmd::ClusterActive(false)]);
    }

    #[tokio::test]
    async fn applying_settings_restarts_the_helper_and_the_sessions() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.drain();
        let restarted = rig.p.helper_restart.clone();
        rig.settings(|c| c.wireless_aa_enabled = true);
        rig.drain();
        assert!(rig.hub.apply_pending());
        rig.p.apply_settings();
        tokio::time::timeout(std::time::Duration::from_secs(1), restarted.notified())
            .await
            .unwrap();
        assert_eq!(rig.drain(), [CpCmd::DropSessions]);
        assert!(!rig.hub.apply_pending());
        assert_eq!(rig.p.apply_due, None);
    }

    async fn restarted(rig: &Rig) -> bool {
        let restart = rig.p.helper_restart.clone();
        tokio::time::timeout(Duration::from_millis(50), restart.notified()).await.is_ok()
    }

    #[tokio::test]
    async fn with_no_phone_a_setting_goes_live_once_the_settings_sit_still() {
        let mut rig = Rig::new();
        rig.settings(|c| c.wireless_aa_enabled = true);
        rig.drain();
        assert!(rig.p.apply_due.is_some());
        rig.p.apply_idle();
        assert!(!rig.hub.apply_pending());
        assert!(restarted(&rig).await);
        assert!(rig.drain().is_empty());

        rig.settings(|c| c.audio_volume = 0.3);
        rig.p.apply_idle();
        assert!(!rig.hub.apply_pending());
        assert!(!restarted(&rig).await);
    }

    #[tokio::test]
    async fn with_a_phone_a_setting_waits_until_the_last_phone_leaves() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.settings(|c| c.wireless_aa_enabled = true);
        assert_eq!(rig.p.apply_due, None);
        rig.p.apply_idle();
        assert!(rig.hub.apply_pending());
        assert!(!restarted(&rig).await);

        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert!(rig.p.apply_due.is_some());
        rig.p.apply_idle();
        assert!(!rig.hub.apply_pending());
        assert!(restarted(&rig).await);
    }

    fn listed(rig: &Rig) -> Vec<livi_core_proto::state::DeviceView> {
        rig.hub.watch().borrow().devices.clone()
    }

    #[tokio::test]
    async fn the_phones_seen_make_the_device_list() {
        use livi_core_proto::state::DeviceStatus;
        use livi_cp::manager::DeviceIds;

        let mut rig = Rig::new();
        rig.connect(1, false);
        rig.device(2, false, "AA:BB:CC:DD:EE:02", "cc:dd:ee:ff:00:02");
        let list = listed(&rig);
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id.as_str(), list[0].status),
            ("aa:bb:cc:dd:ee:02", DeviceStatus::Available)
        );
        assert_eq!((list[0].session, list[0].last_transport.as_deref()), (Some(2), Some("wifi")));

        rig.p.on_event(CpEvent::Presence {
            id: 2,
            wired: false,
            presence: Presence::Status {
                battery_level: Some(50.0),
                battery_charging: Some(true),
                signal_strength: None,
                carrier_name: None,
            },
        });
        assert_eq!(listed(&rig)[0].battery_level, Some(50.0));

        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::Device {
            ids: DeviceIds {
                bt_mac: Some("aa:bb:cc:dd:ee:03".into()),
                usb_udid: Some("u3".into()),
                name: Some("Cable".into()),
                ..Default::default()
            },
        }));
        let cable = listed(&rig).into_iter().find(|d| d.name.as_deref() == Some("Cable")).unwrap();
        assert_eq!(
            (cable.status, cable.last_transport.as_deref()),
            (DeviceStatus::Offline, Some("usb"))
        );
        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::Wifi {
            wifi_mac: "cc:dd:ee:ff:00:02".into(),
            ip: "10.0.0.2".into(),
            connected: true,
        }));

        rig.p.select_device("aa:bb:cc:dd:ee:02");
        assert!(rig.p.sessions[1].active);
        assert_eq!(listed(&rig)[0].status, DeviceStatus::Active);
        rig.p.select_device("nobody");

        rig.drain();
        rig.p.forget_device("aa:bb:cc:dd:ee:02");
        assert!(listed(&rig).iter().all(|d| d.id != "aa:bb:cc:dd:ee:02"));
        tokio::time::sleep(FORGET_GRACE + std::time::Duration::from_millis(100)).await;
        assert!(rig.drain().contains(&CpCmd::Close { id: 2 }));
        rig.p.forget_device("aa:bb:cc:dd:ee:02");

        rig.p.on_event(CpEvent::Disconnected { id: 2 });
        rig.p.on_event(CpEvent::HelperConnected);
        rig.p.on_event(CpEvent::HelperPresence(HelperPresence::Wifi {
            wifi_mac: "cc:dd:ee:ff:00:09".into(),
            ip: String::new(),
            connected: false,
        }));
    }

    #[tokio::test]
    async fn the_camera_comes_to_the_front_while_reversing() {
        let mut rig = Rig::new();
        let mut cfg = rig.p.config_seen.clone();
        cfg.camera_id = "camera-1".into();
        cfg.camera.main = true;
        let car = |v: Value| v.as_object().cloned().unwrap();
        rig.p.on_state(cfg.clone(), car(json!({ "gear": "R" })));
        assert_eq!(rig.front(), Front::Livi, "no camera seen yet");
        rig.p.camera_found(true);
        assert_eq!(rig.front(), Front::Camera);
        rig.p.on_state(cfg.clone(), car(json!({ "gear": "D" })));
        assert_eq!(rig.front(), Front::Livi);

        rig.connect(1, true);
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ringing });
        assert_eq!(rig.front(), Front::Projection);
        rig.p.on_state(cfg.clone(), car(json!({ "gear": "D", "reverse": true })));
        assert_eq!(rig.front(), Front::Camera, "the camera goes over a call");
        rig.want(Front::Livi);
        rig.p.on_state(cfg.clone(), car(json!({ "gear": "D", "reverse": false })));
        assert_eq!(
            rig.front(),
            Front::Projection,
            "the call comes back, the camera page did not end it"
        );

        cfg.auto_switch_on_reverse = false;
        rig.p.on_state(cfg.clone(), car(json!({ "gear": -1 })));
        assert_eq!(rig.front(), Front::Projection);
        assert_eq!(reversing(&car(json!({ "gear": "x" }))), Some(false));
        assert_eq!(reversing(&car(json!({ "speedKph": 3 }))), None);
    }

    #[tokio::test]
    async fn the_phone_hands_the_screen_back() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.want(Front::Projection);
        rig.p.on_event(CpEvent::HostUiRequested { id: 1 });
        assert_eq!(rig.front(), Front::Livi);

        rig.p.on_event(CpEvent::SpeechActive { id: 1, active: true });
        assert_eq!(rig.front(), Front::Projection);
        rig.p.on_event(CpEvent::HostUiRequested { id: 1 });
        assert_eq!(rig.front(), Front::Livi);
        rig.p.on_event(CpEvent::SpeechActive { id: 1, active: false });
        assert_eq!(rig.front(), Front::Livi);
    }

    #[tokio::test]
    async fn telemetry_reaches_carplay_and_only_a_config_change_sets_the_night_mode_again() {
        let mut rig = Rig::new();
        let cfg = rig.p.config_seen.clone();
        let snap = |v: Value| v.as_object().cloned().unwrap_or_default();
        rig.p.on_state(cfg.clone(), snap(json!({ "nightMode": true, "ts": 1 })));
        rig.p.on_state(cfg.clone(), snap(json!({ "nightMode": true, "ts": 2 })));
        assert_eq!(rig.drain(), [CpCmd::NightMode(true)]);

        let mut day = cfg.clone();
        day.appearance_mode = AppearanceMode::Day;
        rig.p.on_state(day, snap(json!({ "nightMode": true, "ts": 2 })));
        assert_eq!(rig.drain(), [CpCmd::InitialNightMode(Some(false))]);

        rig.p.on_event(CpEvent::Spawned { id: 1 });
        assert_eq!(rig.drain(), [CpCmd::NightMode(true)]);
    }

    fn aa(id: AaId, cmd: SessionCmd) -> AaCmd {
        AaCmd::Session { id, cmd }
    }

    fn command(id: AaId, c: Command) -> AaCmd {
        aa(id, SessionCmd::Command(c))
    }

    impl Rig {
        fn aa_connect(&mut self, id: AaId, wired: bool) {
            let usb_serial = wired.then(|| format!("serial{id}"));
            self.p.on_aa(AaEvent::Spawned { id, wired, usb_serial, peer: String::new() });
            self.p.on_aa(AaEvent::Connected { id });
        }
    }

    #[tokio::test]
    async fn android_auto_takes_the_screen_like_carplay() {
        let mut rig = Rig::new();
        rig.aa_connect(5, true);
        assert_eq!(rig.sessions(), (Some(Protocol::Androidauto), 1, 1));
        assert_eq!(rig.p.sessions[0].transport, Transport::Usb);
        let sent = rig.aa_drain();
        assert!(sent.contains(&aa(5, SessionCmd::VideoActive(true))));
        assert!(sent.contains(&aa(5, SessionCmd::Keyframe)));
        assert!(sent.iter().any(|c| matches!(
            c,
            AaCmd::Session { id: 5, cmd: SessionCmd::StreamVolume { kind: AaKind::Media, .. } }
        )));
        assert!(rig.drain().is_empty());

        rig.want(Front::Projection);
        assert_eq!(rig.aa_drain(), [command(5, Command::Home), aa(5, SessionCmd::Keyframe)]);
        rig.p.on_aa(AaEvent::Duck { id: 5, level: 0.2, duration_ms: 300 });
        rig.p.on_aa(AaEvent::Audio {
            id: 5,
            stream: livi_aa_stack::channels::audio::AudioChannelType::Speech,
            sample_rate: 16000,
            channels: 1,
            active: true,
        });
        let levels = rig.aa_drain();
        assert!(matches!(
            levels.as_slice(),
            [
                AaCmd::Session {
                    id: 5,
                    cmd: SessionCmd::StreamVolume { kind: AaKind::Media, ramp_ms: 300, .. }
                },
                AaCmd::Session {
                    id: 5,
                    cmd: SessionCmd::StreamVolume { kind: AaKind::Alert, ramp_ms: 0, .. }
                }
            ]
        ));

        rig.p.on_aa(AaEvent::HostUiRequested { id: 5 });
        assert_eq!(rig.front(), Front::Livi);
        rig.p.on_aa(AaEvent::Disconnected { id: 5 });
        assert_eq!(rig.sessions(), (None, 0, 0));
        rig.p.on_aa(AaEvent::Connected { id: 9 });
        assert_eq!(rig.p.sessions[0].transport, Transport::Wifi);
    }

    #[tokio::test]
    async fn carplay_and_android_auto_share_the_sessions() {
        let mut rig = Rig::new();
        rig.connect(1, true);
        rig.aa_connect(5, false);
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 2));
        rig.drain();
        rig.aa_drain();

        rig.p.activate_next();
        assert_eq!(rig.sessions(), (Some(Protocol::Androidauto), 2, 2));
        assert!(rig.drain().contains(&stack(1, StackCmd::VideoActive(false))));
        assert!(rig.aa_drain().contains(&aa(5, SessionCmd::VideoActive(true))));

        rig.p.on_aa(AaEvent::Disconnected { id: 5 });
        assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
        assert!(rig.drain().contains(&stack(1, StackCmd::VideoActive(true))));
        assert_eq!(rig.aa_drain(), [aa(5, SessionCmd::VideoActive(false))]);
    }

    #[tokio::test]
    async fn android_auto_touches_land_on_its_stream_and_the_voice_key_is_held() {
        let mut rig = Rig::new();
        rig.aa_connect(5, true);
        rig.want(Front::Projection);
        rig.aa_drain();
        rig.crop.send_replace(Some(Crop {
            crop_l: 0.0,
            crop_t: 60.0,
            vis_w: 1280.0,
            vis_h: 600.0,
            tier_w: 1280.0,
            tier_h: 720.0,
        }));
        rig.p.input(Input::Pointer {
            screen: Screen::Main,
            points: vec![
                Point { id: 3, x: 0.5, y: 0.0, phase: Phase::Down },
                Point { id: 4, x: 1.0, y: 1.0, phase: Phase::Cancel },
                Point { id: 5, x: 0.0, y: 0.5, phase: Phase::Move },
            ],
        });
        assert_eq!(
            rig.aa_drain(),
            [aa(
                5,
                SessionCmd::MultiTouch(vec![
                    TouchItem { id: 3, x: 0.5, y: 60.0 / 720.0, action: TouchAction::Down },
                    TouchItem { id: 4, x: 1.0, y: 660.0 / 720.0, action: TouchAction::Up },
                    TouchItem { id: 5, x: 0.0, y: 360.0 / 720.0, action: TouchAction::Move },
                ])
            )]
        );

        rig.p.input(press("KeyV"));
        rig.p.input(Input::Key { code: "KeyV".into(), down: false });
        rig.p.input(Input::Key { code: "KeyN".into(), down: false });
        for code in ["Digit7", "Digit0", "KeyQ", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]
        {
            rig.p.input(press(code));
        }
        for code in ["Backspace", "KeyH", "KeyP", "KeyN", "KeyB", "KeyA", "KeyR", "KeyS"] {
            rig.p.input(press(code));
        }
        rig.p.media(MediaControl::Pause);
        assert_eq!(
            rig.aa_drain(),
            [
                command(5, Command::VoiceAssistant),
                command(5, Command::VoiceAssistantRelease),
                command(5, Command::PhoneKey7),
                command(5, Command::PhoneKey0),
                command(5, Command::Left),
                command(5, Command::Right),
                command(5, Command::Up),
                command(5, Command::Down),
                command(5, Command::Back),
                command(5, Command::Home),
                command(5, Command::PlayPause),
                command(5, Command::Next),
                command(5, Command::Prev),
                command(5, Command::AcceptPhone),
                command(5, Command::RejectPhone),
                command(5, Command::Pause),
            ]
        );

        rig.p.input(press("Enter"));
        assert_eq!(rig.aa_drain(), [command(5, Command::SelectDown)]);
        tokio::time::sleep(SELECT_RELEASE * 2).await;
        assert_eq!(rig.aa_drain(), [command(5, Command::SelectUp)]);
    }

    #[tokio::test]
    async fn android_auto_media_and_guidance_reach_the_state_of_the_phone_in_front() {
        use livi_aa_stack::channels::nav_maneuver::{DrivingSide, ManeuverType};

        let mut rig = Rig::new();
        rig.aa_connect(5, true);
        let first = AaNowPlaying {
            title: Some("Song".into()),
            artist: Some("Band".into()),
            duration_ms: Some(200_000),
            playing: Some(true),
            artwork: Some(vec![1, 2, 3]),
            ..Default::default()
        };
        rig.p.on_aa(AaEvent::NowPlaying { id: 5, now_playing: first });
        let update = AaNowPlaying {
            elapsed_ms: Some(5_000),
            album: Some("Album".into()),
            app: Some("Music".into()),
            artwork: Some(Vec::new()),
            ..Default::default()
        };
        rig.p.on_aa(AaEvent::NowPlaying { id: 5, now_playing: update });
        let np = rig.now_playing();
        assert_eq!((np.title.as_deref(), np.album.as_deref()), (Some("Song"), Some("Album")));
        assert_eq!(
            (np.duration_ms, np.elapsed_ms, np.playing),
            (Some(200_000.0), Some(5_000.0), Some(true))
        );
        assert_eq!((np.artwork.as_deref(), np.app.as_deref()), (Some("AQID"), Some("Music")));

        rig.p.on_aa(AaEvent::NavigationImage { id: 5, image: vec![9] });
        let guidance = AaNavigation {
            active: Some(true),
            app_name: Some("Maps".into()),
            road_name: Some("Main St".into()),
            maneuver_type: Some(ManeuverType::LeftTurn),
            turn_side: Some(DrivingSide::Left),
            turn_angle: Some(90),
            remain_distance: Some(120),
            destination_name: Some("Home".into()),
            distance_to_destination: Some(5_000),
            time_to_destination: Some(600),
            eta: Some("21:58".into()),
            ..Default::default()
        };
        rig.p.on_aa(AaEvent::Navigation { id: 5, navigation: guidance });
        let nav = rig.navigation();
        assert_eq!(nav.maneuver_type, Some(ManeuverType::LeftTurn as u32));
        assert_eq!(
            (nav.turn_side, nav.turn_angle, nav.remain_distance),
            (Some(1), Some(90), Some(120.0))
        );
        assert_eq!(
            (nav.eta_text.as_deref(), nav.app_name.as_deref()),
            (Some("21:58"), Some("Maps"))
        );
        assert_eq!((nav.image.as_deref(), nav.time_to_destination), (Some("CQ=="), Some(600.0)));

        rig.p.on_aa(AaEvent::NavigationImage { id: 5, image: Vec::new() });
        assert_eq!(rig.navigation().image, None);
        let ended =
            AaNavigation { active: Some(false), road_name: Some("x".into()), ..Default::default() };
        rig.p.on_aa(AaEvent::Navigation { id: 5, navigation: ended });
        assert_eq!(rig.navigation(), Navigation { active: Some(false), ..Default::default() });

        rig.p.on_aa(AaEvent::NowPlaying { id: 7, now_playing: AaNowPlaying::default() });
        rig.p.on_aa(AaEvent::Navigation { id: 7, navigation: AaNavigation::default() });
        rig.p.on_aa(AaEvent::NavigationImage { id: 7, image: vec![1] });
    }

    #[tokio::test]
    async fn what_the_phone_in_front_plays_reaches_the_status_file() {
        let mut rig = Rig::new();
        let (tx, seen) = watch::channel(Activity::default());
        rig.p.activity = Some(tx);
        rig.connect(1, true);
        let nav = AudioProfile { kind: AudioKind::Alert, label: "alert".into() };
        rig.p.on_event(CpEvent::AudioActive { id: 1, profile: nav, active: true });
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ringing });
        assert!(seen.borrow().nav_announcing && seen.borrow().speech && seen.borrow().phone);
        rig.p.on_event(CpEvent::Call { id: 1, phase: CallPhase::Ended });
        assert!(!seen.borrow().phone);

        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        assert_eq!(*seen.borrow(), Activity::default());
        rig.aa_connect(5, false);
        rig.p.on_aa(AaEvent::Audio {
            id: 5,
            stream: livi_aa_stack::channels::audio::AudioChannelType::Media,
            sample_rate: 48000,
            channels: 2,
            active: true,
        });
        rig.p.on_aa(helper(json!({ "event": "sco", "up": true })));
        assert!(seen.borrow().media && seen.borrow().phone);
    }

    #[tokio::test]
    async fn the_goodbye_ends_every_session_and_is_answered_after_the_last() {
        let mut rig = Rig::new();
        let (done, heard) = oneshot::channel();
        rig.p.goodbye(done);
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), heard).await.unwrap(), Ok(()));

        rig.connect(1, true);
        rig.aa_connect(5, false);
        rig.drain();
        rig.aa_drain();
        let (done, mut heard) = oneshot::channel();
        rig.p.goodbye(done);
        assert!(rig.drain().contains(&CpCmd::Close { id: 1 }));
        assert!(rig.aa_drain().contains(&AaCmd::Close { id: 5 }));
        rig.p.on_event(CpEvent::Disconnected { id: 1 });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(heard.try_recv().is_err());
        rig.p.on_aa(AaEvent::Disconnected { id: 5 });
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), heard).await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn closing_livi_hands_the_helper_an_empty_paging_list_and_nothing_after() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut rig = Rig::new();
        let helper = tokio::net::UnixListener::bind(rig.dir.0.join("cp-bt.sock")).unwrap();
        let (done, heard) = oneshot::channel();
        rig.p.goodbye(done);
        rig.p.publish();

        let (conn, _) = helper.accept().await.unwrap();
        let (rd, mut wr) = conn.into_split();
        let mut line = String::new();
        tokio::io::BufReader::new(rd).read_line(&mut line).await.unwrap();
        assert_eq!(line, "reconnect-targets []\n");
        wr.write_all(b"{\"ok\":true}\n").await.unwrap();
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), heard).await.unwrap(), Ok(()));
        assert!(tokio::time::timeout(Duration::from_millis(200), helper.accept()).await.is_err());
    }

    #[tokio::test]
    async fn a_seek_reaches_the_carplay_phone_in_front_by_its_bluetooth_mac() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut rig = Rig::new();
        let helper = tokio::net::UnixListener::bind(rig.dir.0.join("cp-bt.sock")).unwrap();
        rig.connect(1, false);
        rig.device(1, false, "AA:BB:CC:DD:EE:FF", "");
        rig.drain();
        rig.p.seek(61000);

        let line = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (conn, _) = helper.accept().await.unwrap();
                let (rd, mut wr) = conn.into_split();
                let mut line = String::new();
                tokio::io::BufReader::new(rd).read_line(&mut line).await.unwrap();
                wr.write_all(b"{\"ok\":true}\n").await.unwrap();
                if line.starts_with("seek ") {
                    return line;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(line, "seek 61000 aa:bb:cc:dd:ee:ff\n");
    }

    fn aa_device(id: AaId, model: &str, instance: &str) -> AaEvent {
        AaEvent::Device {
            id,
            name: "Android".into(),
            model: model.into(),
            instance_id: instance.into(),
            ip: String::new(),
        }
    }

    fn helper(ev: Value) -> AaEvent {
        AaEvent::Helper(ev)
    }

    #[tokio::test]
    async fn an_android_auto_call_on_hands_free_comes_to_the_front_and_gets_its_audio() {
        let mut rig = Rig::new();
        rig.aa_connect(5, false);
        assert_eq!(rig.io_drain(), [Io::FindPhones]);
        let sco = |up: bool| helper(json!({ "event": "sco", "up": up }));
        rig.p.on_aa(sco(true));
        rig.p.on_aa(sco(true));
        assert_eq!(rig.io_drain(), [Io::Call(true)]);
        assert_eq!(rig.front(), Front::Projection);
        rig.p.on_aa(sco(false));
        assert_eq!(rig.io_drain(), [Io::Call(false)]);
        assert_eq!(rig.front(), Front::Livi);
    }

    #[tokio::test]
    async fn the_helper_names_the_android_auto_phone_and_its_link_is_kept() {
        use livi_aa_stack::bridge::PhoneStatus;

        let mut rig = Rig::new();
        let mac = "aa:00:00:00:00:05";
        rig.p.on_aa(helper(json!({
            "event": "aa-device", "btMac": "AA:00:00:00:00:05", "instanceId": "inst5", "usbSerial": ""
        })));
        assert_eq!(rig.io_drain(), [Io::Nudge(mac.into())]);
        rig.aa_connect(5, false);
        assert!(rig.io_drain().is_empty());

        rig.p.on_aa(aa_device(5, "Pixel 9", "inst5"));
        let phone = &listed(&rig)[0];
        assert_eq!((phone.id.as_str(), phone.model.as_deref()), (mac, Some("Pixel 9")));
        assert_eq!((phone.protocol, phone.session), (Some(Protocol::Androidauto), Some(1)));

        let battery = |pct: u32| {
            helper(json!({ "event": "phone-battery", "mac": "AA:00:00:00:00:05", "pct": pct }))
        };
        rig.p.on_aa(battery(60));
        assert_eq!(listed(&rig)[0].battery_level, Some(60.0));
        let status =
            PhoneStatus { battery_level: Some(73), signal_strength: Some(3), ..Default::default() };
        rig.p.on_aa(AaEvent::Status { id: 5, status });
        rig.p.on_aa(battery(40));
        let phone = &listed(&rig)[0];
        assert_eq!((phone.battery_level, phone.signal_strength), (Some(73.0), Some(3.0)));

        rig.p.on_aa(helper(json!({ "event": "hfp", "up": true, "mac": "AA:00:00:00:00:05" })));
        rig.p.keep_links();
        assert!(rig.io_drain().is_empty());
        rig.p.on_aa(helper(json!({ "event": "hfp", "up": false })));
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(121)).await;
        rig.p.keep_links();
        assert_eq!(rig.io_drain(), [Io::Nudge(mac.into())]);
    }

    #[tokio::test]
    async fn the_helper_hears_which_android_auto_phones_are_on_the_cable() {
        let mut rig = Rig::new();
        rig.aa_connect(5, true);
        assert_eq!(rig.io_drain(), [Io::WiredPhones(vec!["SERIAL5".into()]), Io::FindPhones]);
        rig.p.on_aa(aa_device(5, "Pixel", "inst5"));
        let both = vec!["INST5".to_string(), "SERIAL5".into()];
        assert_eq!(rig.io_drain(), [Io::WiredPhones(both.clone())]);
        rig.p.on_aa(AaEvent::HelperConnected);
        assert_eq!(rig.io_drain(), [Io::WiredPhones(both)]);
        rig.p.on_aa(AaEvent::Disconnected { id: 5 });
        assert_eq!(rig.io_drain(), [Io::WiredPhones(Vec::new())]);
    }

    #[tokio::test]
    async fn the_one_connected_phone_is_taken_for_an_unnamed_android_auto_session() {
        let mut rig = Rig::new();
        rig.aa_connect(5, false);
        rig.p.on_aa(aa_device(5, "Pixel", "inst5"));
        assert_eq!(listed(&rig)[0].id, "inst5");
        rig.io_drain();

        rig.p.on_back(Back::Phones(vec!["AA:00:00:00:00:07".into()]));
        let named = rig.p.hands_free.bt_by_instance.get("inst5").map(String::as_str);
        assert_eq!(named, Some("AA:00:00:00:00:07"));
        assert_eq!(rig.io_drain(), [Io::Nudge("aa:00:00:00:00:07".into())]);
        assert_eq!(listed(&rig)[0].id, "aa:00:00:00:00:07");

        rig.p.on_back(Back::Phones(vec!["AA:00:00:00:00:08".into(), "AA:00:00:00:00:09".into()]));
        assert_eq!(rig.p.hands_free.bt_by_instance.len(), 1);
        assert_eq!(rig.io_drain().len(), 2);
    }
}
