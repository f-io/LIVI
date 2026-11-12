// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use std::collections::{BTreeMap, HashMap};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use livi_aa_stack::bridge::{Navigation as AaNavigation, NowPlaying as AaNowPlaying};
use livi_aa_stack::commands::{Command, TouchAction, TouchItem};
use livi_aa_stack::driver::SessionCmd;
use livi_aa_stack::manager::{AaCmd, AaEvent, AaHandle, SessionId as AaId};
use livi_aa_stack::media::AudioKind as AaKind;
use livi_core_proto::config::{Config, KeyBindings};
use livi_core_proto::helper_events::{HelperKind, NowPlayingPatch};
use livi_core_proto::ids::Ids;
use livi_core_proto::input::{Input, Phase, Point};
use livi_core_proto::message::{MediaControl, Radio};
use livi_core_proto::state::{
    Front, Navigation, NowPlaying, PerScreen, Protocol, Screen, Sessions, State, VideoPlayback,
};
use livi_core_proto::theme::{AppearancePreference, Cluster, LayoutSlot, Theme, livi_theme};
use livi_cp::hid::{KnobState, media_button, telephony_button};
use livi_cp::manager::{CallPhase, CpCmd, CpEvent, CpHandle, HelperPresence, Presence, SessionId};
use livi_cp::stack::{AudioKind, StackCmd, Touch};
use livi_cp::ui_sync::{self, Appearance, PhoneChoice};
use livi_media::output::OutputEvent;
use livi_media::planes::{ClusterPlanes, Crop, MainPlane};
use livi_output_proto::{PLANE_AIRPLAY, PLANE_MAIN};
use serde_json::{Map, Value};
use tokio::sync::{Notify, broadcast, mpsc, oneshot, watch};

use crate::audio::spectrum;
use crate::devices::{self, Devices, Seen};
use crate::files::status_file::Activity;
use crate::helper;
use crate::hub::Hub;
use crate::update::UpdateAsk;
use crate::vehicle::aa_telemetry::{self, AaTelemetry};
use crate::vehicle::cp_telemetry::{CpTelemetry, FROM_CARPLAY};
use crate::vehicle::telemetry::{CLIMATE, NOTIFICATION, SOURCE};
use hands_free::{Back, HandsFree, Io};
use theme_catalog::ThemeId;

pub use ap::ApEvent;

mod aa;
pub mod airplay;
pub mod android_auto;
mod ap;
mod audio;
pub mod carplay;
mod cluster;
mod cp;
pub mod cp_radio;
pub mod hands_free;
mod input;
pub mod nav_text;
mod players;
mod screens;
mod sessions;
mod state;
pub mod theme_catalog;

const RAMP_DOWN_MS: u32 = 500;
const RAMP_UP_MS: u32 = 1500;
const KNOB_DEFLECT: f64 = 127.0;
const FORGET_GRACE: Duration = Duration::from_millis(1500);
const SELECT_RELEASE: Duration = Duration::from_millis(200);
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
    Ap(u64),
}

impl Link {
    fn protocol(self) -> Protocol {
        match self {
            Link::Cp(_) => Protocol::Carplay,
            Link::Aa(_) => Protocol::Androidauto,
            Link::Ap(_) => Protocol::Airplay,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Do {
    Keyframe,
    Home,
    Media(MediaControl),
    Volume {
        kind: AudioKind,
        level: f64,
        ramp_ms: u32,
    },
    VideoActive(bool),
    ScreenOff,
    Close,
    PowerDown,
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
    video: bool,
    duck_level: f64,
    duck_ramp_ms: u32,
    now_playing: NowPlaying,
    video_playback: Option<VideoPlayback>,
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
    ClusterLayout,
    ClusterContent,
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

    fn media_control(self) -> Option<MediaControl> {
        Some(match self {
            Key::PlayPause => MediaControl::PlayPause,
            Key::Play => MediaControl::Play,
            Key::Pause => MediaControl::Pause,
            Key::Next => MediaControl::Next,
            Key::Prev => MediaControl::Prev,
            _ => return None,
        })
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
        (&b.cluster_layout, Key::ClusterLayout),
        (&b.cluster_content, Key::ClusterContent),
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
    table
        .into_iter()
        .find(|(c, _)| !c.is_empty() && c.as_str() == code)
        .map(|(_, key)| key)
}

fn reversing(telemetry: &Map<String, Value>) -> Option<bool> {
    match telemetry.get("reverse").and_then(Value::as_bool) {
        Some(reverse) => Some(reverse),
        None => telemetry
            .get("gear")
            .map(|g| aa_telemetry::gear(Some(g), None) == Some(aa_telemetry::REVERSE)),
    }
}

pub fn start_page(cfg: &Config, last: Option<&str>) -> String {
    if cfg.start_page == "last" {
        last.unwrap_or("/").to_string()
    } else {
        cfg.start_page.clone()
    }
}

pub fn first_wants(start: &str) -> PerScreen<Front> {
    PerScreen {
        main: if start == "/" {
            Front::Projection
        } else {
            Front::Livi
        },
        cluster: Front::Livi,
        aux: Front::Livi,
    }
}

fn shows_cluster(wants: &PerScreen<Front>) -> bool {
    [wants.main, wants.cluster, wants.aux].contains(&Front::Cluster)
}

fn knob(state: KnobState) -> StackCmd {
    StackCmd::Knob {
        state,
        momentary: true,
    }
}

fn aa_open(target: &str) -> Option<Command> {
    Some(match target.split([':', '/']).next().unwrap_or_default() {
        "maps" | "comgooglemaps" | "waze" => Command::OpenNavigation,
        "nowplaying" | "music" | "podcasts" | "audiobooks" => Command::OpenMedia,
        "mobilephone" | "tel" => Command::OpenPhone,
        "radio" => Command::OpenRadio,
        _ => return None,
    })
}

pub struct Drivers {
    pub cp: CpHandle,
    pub aa: AaHandle,
    pub aa_crop: watch::Receiver<Option<Crop>>,
    pub phone_io: mpsc::UnboundedSender<Io>,
    pub phone_back: mpsc::UnboundedReceiver<Back>,
    pub telemetry: mpsc::UnboundedSender<Value>,
    pub settings: mpsc::UnboundedSender<Value>,
    pub themes: PathBuf,
}

pub struct Projection {
    hub: Arc<Hub>,
    cp: CpHandle,
    aa: AaHandle,
    aa_crop: watch::Receiver<Option<Crop>>,
    phone_io: mpsc::UnboundedSender<Io>,
    phone_back: Option<mpsc::UnboundedReceiver<Back>>,
    telemetry: mpsc::UnboundedSender<Value>,
    settings: mpsc::UnboundedSender<Value>,
    hands_free: HandsFree,
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
    asked: PerScreen<Front>,
    attention: Option<(Attention, Front)>,
    bindings: KeyBindings,
    config_seen: Config,
    telemetry_seen: Map<String, Value>,
    reversing: bool,
    camera_found: bool,
    cp_telemetry: CpTelemetry,
    aa_telemetry: AaTelemetry,
    bye: Option<oneshot::Sender<()>>,
    activity: Option<watch::Sender<Activity>>,
    apply_due: Option<tokio::time::Instant>,
    cluster: Cluster,
    themes: PathBuf,
    theme_asked: String,
    theme_ready: (mpsc::UnboundedSender<Theme>, mpsc::UnboundedReceiver<Theme>),
    radio: Option<mpsc::UnboundedSender<crate::radio::RadioAsk>>,
    players: players::Players,
    ap_events: Option<mpsc::UnboundedReceiver<ApEvent>>,
    ap_peers: HashMap<u64, ap::ApPeer>,
    ap_audio: Option<Arc<crate::projection::airplay::ApAudio>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    Media(MediaControl),
    Seek(u32),
    VideoPlay(bool),
    VideoSeek(u32),
    VideoClose,
    NextDevice,
    SelectClusterLayout(String),
    StepClusterLayout(i32),
    MoveClusterLayout(String, i32),
    ShowClusterLayout(String, bool),
    ApplySettings,
    SelectDevice(String),
    ForgetDevice(String),
    CameraFound(bool),
    OpenApp(String),
}

fn kept_cluster(cfg: &Config, theme: Theme) -> Cluster {
    Cluster::new(
        theme,
        &cfg.cluster_layout,
        &cfg.cluster_palettes,
        &cfg.dashboard_layouts,
    )
}

pub struct UiAsks {
    pub wants: watch::Receiver<PerScreen<Front>>,
    pub commands: mpsc::UnboundedReceiver<UiCommand>,
    pub input: mpsc::UnboundedReceiver<Input>,
    pub spectrum: Arc<spectrum::Feed>,
    pub goodbye: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    pub activity: watch::Sender<Activity>,
    pub radio: Option<crate::radio::RadioAsks>,
    pub radio_from_phone: mpsc::UnboundedSender<crate::radio::RadioAsk>,
    pub input_from_compositor: mpsc::UnboundedSender<Input>,
    pub commands_back: mpsc::UnboundedSender<UiCommand>,
    pub ap_ready: watch::Receiver<bool>,
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
    pub radio: mpsc::UnboundedSender<crate::radio::RadioAsk>,
    pub ap_ready: watch::Sender<bool>,
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
    let (radio_tx, radio_rx) = mpsc::unbounded_channel();
    let (ap_ready_tx, ap_ready_rx) = watch::channel(true);
    let feed = spectrum::Feed::new();
    (
        UiSenders {
            wants: wants_tx,
            commands: commands_tx.clone(),
            input: input_tx.clone(),
            dongle: dongle_tx,
            link_speed_viewers: viewers_tx,
            spectrum: feed.clone(),
            goodbye: goodbye_tx,
            update: update_tx,
            activity: activity_rx,
            radio: radio_tx.clone(),
            ap_ready: ap_ready_tx,
        },
        UiAsks {
            wants: wants_rx,
            commands: commands_rx,
            input: input_rx,
            spectrum: feed,
            goodbye: goodbye_rx,
            activity: activity_tx,
            radio: Some(radio_rx),
            radio_from_phone: radio_tx,
            input_from_compositor: input_tx,
            commands_back: commands_tx,
            ap_ready: ap_ready_rx,
        },
        DongleAsks {
            radios: dongle_rx,
            link_speed_viewers: viewers_rx,
        },
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
        let Drivers {
            cp,
            aa,
            aa_crop,
            phone_io,
            phone_back,
            telemetry,
            settings,
            themes,
        } = drivers;
        let cp_telemetry = CpTelemetry::new(devices.helper.clone());
        let cfg = hub.config();
        cp.send(CpCmd::NightModes(cfg.night_modes()));
        let cluster_on = shows_cluster(&wants);
        cp.send(CpCmd::ClusterActive(cluster_on));
        aa.send(AaCmd::ClusterActive(cluster_on));
        let theme = theme_catalog::cached(&themes, &cfg.car_play_theme).unwrap_or_else(livi_theme);
        let cluster = kept_cluster(&cfg, theme);
        Self {
            cp,
            aa,
            aa_crop,
            phone_io,
            phone_back: Some(phone_back),
            telemetry,
            settings,
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
            cp_telemetry,
            aa_telemetry: AaTelemetry::default(),
            bye: None,
            activity: None,
            apply_due: None,
            cluster,
            themes,
            theme_asked: String::new(),
            theme_ready: mpsc::unbounded_channel(),
            radio: None,
            players: players::Players::default(),
            ap_events: None,
            ap_peers: HashMap::new(),
            ap_audio: None,
            hub,
        }
    }

    pub fn hear_airplay(
        &mut self,
        events: mpsc::UnboundedReceiver<ApEvent>,
        audio: Arc<crate::projection::airplay::ApAudio>,
    ) {
        self.ap_events = Some(events);
        self.ap_audio = Some(audio);
    }

    pub async fn run(
        mut self,
        mut events: mpsc::UnboundedReceiver<CpEvent>,
        mut aa_events: mpsc::UnboundedReceiver<AaEvent>,
        mut asks: UiAsks,
        mut state: watch::Receiver<State>,
    ) {
        let mut created = Some(self.plane.player_created());
        let mut host = Some(self.plane.host_events());
        let mut cluster_created = Some(self.clusters.player_created());
        let mut back = self
            .phone_back
            .take()
            .unwrap_or_else(|| mpsc::unbounded_channel().1);
        let mut themes = std::mem::replace(&mut self.theme_ready.1, mpsc::unbounded_channel().1);
        let mut ap_events = self
            .ap_events
            .take()
            .unwrap_or_else(|| mpsc::unbounded_channel().1);
        self.follow_theme(&self.config_seen.clone());
        self.devices.ap_ready(*asks.ap_ready.borrow_and_update());
        self.activity = Some(asks.activity);
        self.radio = Some(asks.radio_from_phone);
        let mut keep = tokio::time::interval(hands_free::KEEP_EVERY);
        keep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(ev) => self.on_event(ev),
                    None => return,
                },
                Some(ev) = aa_events.recv() => self.on_aa(ev),
                Some(ev) = ap_events.recv() => self.on_ap(ev),
                Some(answer) = back.recv() => self.on_back(answer),
                Some(theme) = themes.recv() => self.theme_fetched(theme),
                _ = keep.tick() => self.keep_links(),
                changed = asks.wants.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let wants = *asks.wants.borrow_and_update();
                    self.asked(wants);
                }
                changed = asks.ap_ready.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let ready = *asks.ap_ready.borrow_and_update();
                    if self.devices.ap_ready(ready) {
                        let targets = self.targets();
                        self.devices.page(targets, true);
                    }
                }
                Some(command) = asks.commands.recv() => match command {
                    UiCommand::Media(control) => self.media(control),
                    UiCommand::Seek(ms) => self.seek(ms),
                    UiCommand::VideoPlay(playing) => self.to_video(StackCmd::VideoPlay(playing)),
                    UiCommand::VideoSeek(ms) => {
                        self.to_video(StackCmd::VideoSeek(f64::from(ms) / 1000.0));
                    }
                    UiCommand::VideoClose => self.to_video(StackCmd::VideoClose),
                    UiCommand::NextDevice => self.activate_next(),
                    UiCommand::SelectClusterLayout(layout) => {
                        if self.cluster.select(&layout, None) {
                            self.cluster_changed();
                        }
                    }
                    UiCommand::StepClusterLayout(by) => {
                        if self.cluster.step(by) {
                            self.cluster_changed();
                        }
                    }
                    UiCommand::MoveClusterLayout(layout, by) => {
                        self.arrange_dashboard(self.cluster.moved(&layout, by));
                    }
                    UiCommand::ShowClusterLayout(layout, on) => {
                        self.arrange_dashboard(self.cluster.switched(&layout, on));
                    }
                    UiCommand::ApplySettings => self.apply_settings(),
                    UiCommand::SelectDevice(id) => self.select_device(&id),
                    UiCommand::ForgetDevice(id) => self.forget_device(&id),
                    UiCommand::CameraFound(found) => self.camera_found(found),
                    UiCommand::OpenApp(target) => self.open_app(&target),
                },
                Some(input) = asks.input.recv() => self.input(input),
                Some(done) = asks.goodbye.recv() => self.goodbye(done),
                () = until(self.apply_due) => self.apply_idle(),
                changed = state.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let (cfg, telemetry, phone, radio) = {
                        let s = state.borrow_and_update();
                        (
                            s.config.clone(),
                            s.telemetry.clone(),
                            s.now_playing.playing == Some(true),
                            s.dab.playing || s.fm.playing,
                        )
                    };
                    self.on_state(cfg, telemetry);
                    self.follow_players(phone, radio);
                }
                made = next_of(&mut created) => match made {
                    Ok(()) => self.keyframe(),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => created = None,
                },
                ev = next_of(&mut host) => match ev {
                    Ok(OutputEvent::Started { plane: PLANE_MAIN }) => {
                        println!("[core] main video: first frame at the decoder");
                        self.main_video_shown();
                    }
                    Ok(OutputEvent::Started { plane: PLANE_AIRPLAY }) => {
                        println!("[core] airplay video: first frame at the decoder");
                        self.ap_video_shown();
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[core] missed {n} output events");
                    }
                    Err(broadcast::error::RecvError::Closed) => host = None,
                },
                made = next_of(&mut cluster_created) => match made {
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
                Do::Home => self.cp.stack(
                    id,
                    knob(KnobState {
                        home: true,
                        ..Default::default()
                    }),
                ),
                Do::Media(control) => self.cp.stack(id, StackCmd::Media(media_index(control))),
                Do::Volume {
                    kind,
                    level,
                    ramp_ms,
                } => {
                    self.cp.stack(
                        id,
                        StackCmd::StreamVolume {
                            kind,
                            level,
                            ramp_ms,
                        },
                    );
                }
                Do::VideoActive(on) => {
                    self.cp.stack(id, StackCmd::VideoActive(on));
                    self.cp.stack(id, StackCmd::AudioActive(on));
                }
                Do::ScreenOff => self.cp.stack(id, StackCmd::VideoActive(false)),
                Do::Close | Do::PowerDown => self.cp.send(CpCmd::Close { id }),
            },
            Link::Aa(id) => match what {
                Do::Keyframe => self.aa.session(id, SessionCmd::Keyframe),
                Do::Home => self.aa.session(id, SessionCmd::Command(Command::Home)),
                Do::Media(control) => {
                    self.aa
                        .session(id, SessionCmd::Command(media_command(control)));
                }
                Do::Volume {
                    kind,
                    level,
                    ramp_ms,
                } => {
                    self.aa.session(
                        id,
                        SessionCmd::StreamVolume {
                            kind: aa_kind(kind),
                            level,
                            ramp_ms,
                        },
                    );
                }
                Do::VideoActive(on) => self.aa.session(id, SessionCmd::VideoActive(on)),
                Do::ScreenOff => self.aa.session(id, SessionCmd::VideoActive(false)),
                Do::Close => self.aa.send(AaCmd::Close { id }),
                Do::PowerDown => self.aa.session(id, SessionCmd::PowerDown),
            },
            Link::Ap(id) => match what {
                Do::Close | Do::PowerDown => {
                    if let Some(peer) = self.ap_peers.get(&id) {
                        peer.close.notify_one();
                    }
                }
                Do::Media(control) => self.ap_media(id, control),
                Do::Keyframe
                | Do::Home
                | Do::Volume { .. }
                | Do::VideoActive(_)
                | Do::ScreenOff => {}
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
}

fn written_out(nav: &mut Navigation, language: nav_text::Language) {
    nav.maneuver_text = nav
        .maneuver_type
        .and_then(|code| nav_text::maneuver(code, language));
    nav.maneuver_distance_text = nav.remain_distance.and_then(nav_text::distance);
    nav.destination_distance_text = nav.distance_to_destination.and_then(nav_text::distance);
    nav.time_left_text = nav.time_to_destination.and_then(nav_text::time_left);
    for m in &mut nav.maneuvers {
        m.maneuver_text = m
            .maneuver_type
            .and_then(|code| nav_text::maneuver(code, language));
    }
}

async fn next_of<T: Clone>(
    rx: &mut Option<broadcast::Receiver<T>>,
) -> Result<T, broadcast::error::RecvError> {
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
    if wired {
        Transport::Usb
    } else {
        Transport::Wifi
    }
}

#[cfg(test)]
mod tests;
