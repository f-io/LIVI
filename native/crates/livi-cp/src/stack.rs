// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::unix::{OwnedReadHalf as UnixRead, OwnedWriteHalf as UnixWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};

use crate::auth_setup;
use crate::bplist::{self, Value, dict};
use crate::control_cipher::ControlCipher;
use crate::crypto::hkdf_sha512;
use crate::helper_sock::HelperSock;
use crate::hid::{self, Contact, KnobState};
use crate::hls::Client;
use crate::hls_gateway::Gateway;
use crate::iap_tunnel::IapTunnel;
use crate::identity::Identity;
use crate::info::{self, InfoConfig};
use crate::keep_alive::KeepAlive;
use crate::media::{AudioCodec, AudioRequest, Media, MicRequest};
use crate::mfi4;
use crate::net;
use crate::pair_setup::PairSetup;
use crate::pair_verify::PairVerify;
use crate::pairings::Pairings;
use crate::rtsp::{self, Request, Response};
use crate::timing::ntp64_now;
use crate::timing_sync::TimingSync;
use crate::ui_sync;
use crate::vehicle_state::{
    self, Climate, Gauges, HistoryEntry, Lane, Notice, Radio, RadioCommand, VehicleState,
};
use crate::video_playback;
use crate::video_proxy::ProxyOffer;

mod commands;
mod control;
mod data_streams;
mod events;
mod feedback;
mod setup;
mod teardown;
mod video;

const STREAM_MAIN_SCREEN: u64 = 110;
const STREAM_ALT_SCREEN: u64 = 111;
const STREAM_MAIN_AUDIO: u64 = 100;
const STREAM_ALT_AUDIO: u64 = 101;
const STREAM_MAIN_HIGH_AUDIO: u64 = 102;
const STREAM_DATA: u64 = 130;
const IAP_DATASTREAM_UUID: &str = "E9459FD0-BCAD-4C45-820F-1E72447EF2F2";

const PHONE_SILENT: Duration = Duration::from_secs(10);
const SILENCE_CHECK: Duration = Duration::from_millis(250);

const CLUSTER_MAP_URL: &str = "maps:/car/instrumentcluster/map";
const APPEARANCE_AUTOMATIC: u64 = 0;

const AAC_LC_44K_STEREO: u64 = 0x40_0000;
const AAC_LC_48K_STEREO: u64 = 0x80_0000;
const OPUS_16K: u64 = 0x1000_0000;
const OPUS_24K: u64 = 0x2000_0000;
const OPUS_48K: u64 = 0x4000_0000;
const OPUS_MONO: u64 = OPUS_16K | OPUS_24K | OPUS_48K;

const PLIST: &str = "application/x-apple-binary-plist";
const PAIRING: &str = "application/pairing+tlv8";
const OCTET: &str = "application/octet-stream";

fn pcm_format(fmt: u64) -> Option<(u32, u8)> {
    Some(match fmt {
        0x4 => (8000, 1),
        0x8 => (8000, 2),
        0x10 => (16000, 1),
        0x20 => (16000, 2),
        0x40 => (24000, 1),
        0x80 => (24000, 2),
        0x100 => (32000, 1),
        0x200 => (32000, 2),
        0x400 => (44100, 1),
        0x800 => (44100, 2),
        0x4000 => (48000, 1),
        0x8000 => (48000, 2),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioKind {
    Speech = 1,
    Call = 2,
    Media = 3,
    Alert = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProfile {
    pub kind: AudioKind,
    pub label: String,
}

fn audio_profile(audio_type: &str) -> AudioProfile {
    let (kind, label) = match audio_type {
        "telephony" => (AudioKind::Call, "telephony"),
        "speechRecognition" => (AudioKind::Speech, "speech"),
        "media" => (AudioKind::Media, "media"),
        "" => (AudioKind::Alert, "nav"),
        other => (AudioKind::Alert, other),
    };
    AudioProfile {
        kind,
        label: label.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCodec {
    H264,
    H265,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StackConfig {
    pub info: InfoConfig,
    pub audio_device: String,
    pub audio_input_device: String,
    pub hls_video: bool,
    pub vehicle_climate: bool,
    pub app_version: String,
    pub app_build_date: String,
    pub cluster_url: String,
    pub vehicle_variant: String,
    pub engines: vehicle_state::Engines,
    pub cluster_content: Option<vehicle_state::ClusterContent>,
    pub images: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    pub x: f64,
    pub y: f64,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StackCmd {
    NextClusterContent,
    ClusterLayout(ui_sync::Layout),
    Touches(Vec<Touch>),
    Knob {
        state: KnobState,
        momentary: bool,
    },
    KnobSelect(bool),
    Notification(Option<Notice>),
    History(Vec<HistoryEntry>),
    Media(u8),
    Telephony(u8),
    Siri,
    NightMode(bool),
    ClusterNightMode(bool),
    LimitedUi(bool),
    VideoAllowed(bool),
    Keyframe,
    ClusterActive(bool),
    VideoActive(bool),
    AudioActive(bool),
    StreamVolume {
        kind: AudioKind,
        level: f64,
        ramp_ms: u32,
    },
    PhoneBtMac(String),
    Climate(Climate),
    Gauges(Gauges),
    Radio(Box<Radio>),
    RequestUi(String),
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StackEvent {
    DeviceInfo {
        name: String,
        device_id: String,
        wifi_mac: String,
        model: String,
    },
    Active {
        ip: String,
        controller_id: Option<String>,
    },
    VideoCodec {
        cluster: bool,
        codec: ScreenCodec,
    },
    MainScreenReady,
    AudioActive {
        profile: AudioProfile,
        active: bool,
    },
    MicActive {
        active: bool,
        rate: u32,
        channels: u8,
    },
    Duck {
        level: f64,
        duration_ms: u32,
    },
    HostUiRequested,
    SpeechActive(bool),
    DisableBluetooth(String),
    Climate(Climate),
    Radio(RadioCommand),
    Volume {
        name: &'static str,
        level: f64,
    },
    Cluster(ui_sync::PhoneChoice),
    Closed {
        was_active: bool,
    },
    NotificationClosed,
    HistoryHidden(u64),
}

pub struct StackCtx<M: Media> {
    pub identity: Identity,
    pub pairings: Pairings,
    pub helper: HelperSock,
    pub media: Arc<M>,
    pub config: watch::Receiver<StackConfig>,
    pub refresh: Option<Arc<dyn Fn() + Send + Sync>>,
    pub debug: bool,
    pub video_capture: Option<std::path::PathBuf>,
}

struct AudioStreamState {
    stream_type: u64,
    sample_rate: u32,
    connection_id: Value,
    host_stream: u32,
    mic: Option<u32>,
    origin: Option<(u32, Instant)>,
    playout_latency_ms: u32,
    profile: AudioProfile,
    mic_plan: Option<([u8; 32], MicRequest)>,
}

struct EventConn {
    read: OwnedReadHalf,
    write: OwnedWriteHalf,
    cipher: ControlCipher,
    sealed: Vec<u8>,
    plain: Vec<u8>,
    cseq: u32,
}

struct Conn<M: Media> {
    ctx: Arc<StackCtx<M>>,
    events: mpsc::UnboundedSender<StackEvent>,
    peer: SocketAddr,
    pair_setup: PairSetup,
    pair_verify: PairVerify,
    mfi4: Option<mfi4::Session>,
    cipher: Option<ControlCipher>,
    sealed: Vec<u8>,
    plain: Vec<u8>,
    device_bt_mac: String,
    phone_bt_mac: String,
    timing: Option<TimingSync>,
    keep_alive: Option<KeepAlive>,
    last_request: tokio::time::Instant,
    screen: Option<u32>,
    cluster_screen: Option<u32>,
    codec_emitted: bool,
    cluster_codec_emitted: bool,
    main_stream_ready: bool,
    audio: Vec<AudioStreamState>,
    iap_tunnel: Option<IapTunnel>,
    iap_out: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    iap_relay: Option<(UnixRead, UnixWrite)>,
    video_playback: bool,
    video_capture: Option<video_playback::Capture>,
    last_stream_id: u64,
    video_settings: Option<IapTunnel>,
    video_settings_out: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    video_overlay: Option<IapTunnel>,
    video_overlay_out: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    ui_sync_on: bool,
    ui_sync: Option<(IapTunnel, mpsc::UnboundedSender<Vec<u8>>)>,
    ui_sync_out: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    ui_sync_state: ui_sync::UiSync,
    vehicle_state: Option<VehicleState>,
    notice: Option<Notice>,
    notice_ready_at: Option<tokio::time::Instant>,
    notice_ready: bool,
    notice_shown: bool,
    select_closed_notice: bool,
    offered_vehicle_state: bool,
    climate_reported: Option<Climate>,
    volume_reported: Option<Vec<(&'static str, f64)>>,
    vehicle_streams: Vec<(Lane, IapTunnel, mpsc::UnboundedSender<Vec<u8>>)>,
    vehicle_in: Option<mpsc::UnboundedSender<(Lane, Vec<u8>)>>,
    vehicle_out: Option<mpsc::UnboundedReceiver<(Lane, Vec<u8>)>>,
    video_screen_borrowed: bool,
    video_allowed: bool,
    video_proxy: Option<Arc<ProxyOffer>>,
    video_tunnel: Option<VideoRun>,
    event_listener: Option<TcpListener>,
    event: Option<EventConn>,
    live: bool,
    night_mode: Option<bool>,
    cluster_night_mode: Option<bool>,
    limited_ui: Option<bool>,
    cluster_want: bool,
    cluster_url: Option<String>,
    video_active: bool,
    audio_active: bool,
    call_active: bool,
    speech_active: bool,
    video_player: video_playback::Playback,
    video_muted: Arc<AtomicBool>,
}

struct VideoRun {
    _gateway: Gateway,
    restarts: tokio::task::JoinHandle<()>,
    stop: Box<dyn Fn() + Send + Sync>,
}

impl Drop for VideoRun {
    fn drop(&mut self) {
        self.restarts.abort();
        (self.stop)();
    }
}

const VIDEO_REOPEN_PAUSE: Duration = Duration::from_millis(300);

async fn reopen_video<M: Media>(
    media: Arc<M>,
    url: String,
    device: String,
    muted: Arc<AtomicBool>,
    mut started_over: mpsc::UnboundedReceiver<()>,
) {
    while started_over.recv().await.is_some() {
        println!("[cpStack] video stream started over, opening it again");
        media.stop_video();
        tokio::time::sleep(VIDEO_REOPEN_PAUSE).await;
        media.play_video(url.clone(), device.clone(), muted.load(Ordering::Relaxed));
    }
}

fn sounding_sample(first: u32, since_first_s: f64, lag_ms: u32, rate: u32) -> u32 {
    let played = (since_first_s - f64::from(lag_ms) / 1000.0).max(0.0);
    (i64::from(first) + (played * f64::from(rate) + 0.5).floor() as i64) as u32
}

async fn accept(listener: &Option<TcpListener>) -> io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(l) => l.accept().await,
        None => std::future::pending().await,
    }
}

async fn read_event(event: &mut Option<EventConn>, buf: &mut [u8]) -> io::Result<usize> {
    match event {
        Some(e) => e.read.read(buf).await,
        None => std::future::pending().await,
    }
}

async fn read_relay(
    relay: &mut Option<(UnixRead, UnixWrite)>,
    buf: &mut [u8],
) -> io::Result<usize> {
    match relay {
        Some((rd, _)) => rd.read(buf).await,
        None => std::future::pending().await,
    }
}

async fn recv_iap<T>(out: &mut Option<mpsc::UnboundedReceiver<T>>) -> Option<T> {
    match out {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_started(
    rx: &mut Option<broadcast::Receiver<(u32, u32)>>,
) -> Result<(u32, u32), broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

const NOTICE_DELAY: Duration = Duration::from_secs(5);

async fn sleep_until(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn id_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Int(n)) => n.to_string(),
        Some(Value::Real(f)) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", *f as i64),
        Some(Value::Real(f)) => f.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => "undefined".to_string(),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Int(n)) => *n != 0,
        Some(Value::Real(f)) => *f != 0.0 && !f.is_nan(),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        other => id_text(Some(other)),
    }
}

fn number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Int(n)) => *n as f64,
        Some(Value::Real(f)) => *f,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn int(v: Option<&Value>) -> u64 {
    let n = number(v);
    if n.is_finite() && n > 0.0 {
        n as u64
    } else {
        0
    }
}

fn plist_response(body: &Value) -> Response {
    Response {
        headers: vec![("Content-Type".into(), PLIST.into())],
        body: bplist::encode(body),
        ..Default::default()
    }
}

fn status(code: u16) -> Response {
    Response {
        status: Some(code),
        ..Default::default()
    }
}

fn octet(result: Result<Vec<u8>, String>, what: &str) -> Response {
    match result {
        Ok(body) => Response {
            headers: vec![("Content-Type".into(), OCTET.into())],
            body,
            ..Default::default()
        },
        Err(e) => {
            println!("[cpStack] {what}: {e}");
            status(500)
        }
    }
}

fn header_number(req: &Request, name: &str) -> Option<u32> {
    let v = req.headers.get(name)?.trim();
    if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if v.len() > 1 && v.starts_with('0') {
        u32::from_str_radix(&v[1..], 8).ok()
    } else {
        v.parse().ok()
    }
}

fn stream_key(shared: &[u8; 32], id: &str, info: &str) -> [u8; 32] {
    hkdf_sha512(
        shared,
        format!("DataStream-Salt{id}").as_bytes(),
        info.as_bytes(),
    )
}

pub async fn run<M: Media>(
    stream: TcpStream,
    ctx: Arc<StackCtx<M>>,
    mut cmds: mpsc::UnboundedReceiver<StackCmd>,
    events: mpsc::UnboundedSender<StackEvent>,
) {
    let peer = stream
        .peer_addr()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
    println!("[cpStack] control connection from {peer}");
    let mut started = Some(ctx.media.audio_started());
    let (mut rd, mut wr) = stream.into_split();
    let vehicle_state = {
        let cfg = ctx.config.borrow();
        VehicleState::new(ctx.debug, &cfg.app_version, &cfg.app_build_date)
            .with_climate(cfg.vehicle_climate)
            .with_variant(&cfg.vehicle_variant)
            .with_engines(cfg.engines)
            .with_cluster_content(cfg.cluster_content.as_ref())
            .with_images(&cfg.images)
    };
    let mut conn = Conn {
        ctx,
        events,
        peer,
        pair_setup: PairSetup::new(),
        pair_verify: PairVerify::new(),
        mfi4: None,
        cipher: None,
        sealed: Vec::new(),
        plain: Vec::new(),
        device_bt_mac: String::new(),
        phone_bt_mac: String::new(),
        timing: None,
        keep_alive: None,
        last_request: tokio::time::Instant::now(),
        screen: None,
        cluster_screen: None,
        codec_emitted: false,
        cluster_codec_emitted: false,
        main_stream_ready: false,
        audio: Vec::new(),
        iap_tunnel: None,
        iap_out: None,
        iap_relay: None,
        video_playback: false,
        video_capture: None,
        last_stream_id: 1,
        video_settings: None,
        video_settings_out: None,
        video_overlay: None,
        video_overlay_out: None,
        ui_sync_on: false,
        ui_sync: None,
        ui_sync_out: None,
        ui_sync_state: ui_sync::UiSync::default(),
        vehicle_state: Some(vehicle_state),
        offered_vehicle_state: false,
        notice: None,
        notice_ready_at: None,
        notice_ready: false,
        notice_shown: false,
        select_closed_notice: false,
        climate_reported: None,
        volume_reported: None,
        vehicle_streams: Vec::new(),
        vehicle_in: None,
        vehicle_out: None,
        video_screen_borrowed: false,
        video_allowed: true,
        video_proxy: None,
        video_tunnel: None,
        event_listener: None,
        event: None,
        live: false,
        night_mode: None,
        cluster_night_mode: None,
        limited_ui: None,
        cluster_want: false,
        cluster_url: None,
        video_active: false,
        audio_active: false,
        call_active: false,
        speech_active: false,
        video_player: video_playback::Playback::default(),
        video_muted: Arc::new(AtomicBool::new(false)),
    };
    let mut buf = vec![0u8; 64 * 1024];
    let mut event_buf = vec![0u8; 16 * 1024];
    let mut relay_buf = vec![0u8; 16 * 1024];
    let mut stopped = false;
    let mut silence = tokio::time::interval(SILENCE_CHECK);
    let mut config = conn.ctx.config.clone();
    config.mark_unchanged();
    loop {
        tokio::select! {
            Ok(()) = config.changed() => conn.follow_cluster_url().await,
            read = rd.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    conn.last_request = tokio::time::Instant::now();
                    if conn.on_control_bytes(&buf[..n], &mut wr).await.is_err() {
                        break;
                    }
                }
            },
            _ = sleep_until(conn.notice_ready_at), if conn.notice_ready_at.is_some() => {
                conn.notice_ready_at = None;
                conn.notice_ready = true;
                conn.show_notice();
            }
            _ = silence.tick() => {
                if conn.went_silent() {
                    println!(
                        "[cpStack] the phone has been silent for {} s, the session ends",
                        PHONE_SILENT.as_secs()
                    );
                    break;
                }
            }
            cmd = cmds.recv() => match cmd {
                None | Some(StackCmd::Stop) => {
                    stopped = true;
                    break;
                }
                Some(cmd) => conn.on_cmd(cmd).await,
            },
            accepted = accept(&conn.event_listener) => {
                if let Ok((sock, _)) = accepted {
                    conn.on_event_connection(sock);
                }
            }
            read = read_event(&mut conn.event, &mut event_buf) => match read {
                Ok(0) | Err(_) => {
                    println!("[cpStack] event channel closed");
                    conn.event = None;
                }
                Ok(n) => conn.on_event_bytes(&event_buf[..n]).await,
            },
            read = read_relay(&mut conn.iap_relay, &mut relay_buf) => match read {
                Ok(0) | Err(_) => conn.iap_relay = None,
                Ok(n) => {
                    let data = relay_buf[..n].to_vec();
                    conn.send_event_command(&dict([
                        ("type", Value::String("iAPSendMessage".into())),
                        ("params", dict([("data", Value::Data(data))])),
                    ]))
                    .await;
                }
            },
            iap = recv_iap(&mut conn.iap_out) => match iap {
                Some(iap) => conn.relay_iap(&iap).await,
                None => conn.iap_out = None,
            },
            sync = recv_iap(&mut conn.ui_sync_out) => match sync {
                Some(message) => conn.on_ui_sync(&message),
                None => conn.ui_sync_out = None,
            },
            overlay = recv_iap(&mut conn.video_overlay_out) => match overlay {
                Some(overlay) => conn.keep_side_message("overlay", &overlay),
                None => conn.video_overlay_out = None,
            },
            setting = recv_iap(&mut conn.video_settings_out) => match setting {
                Some(setting) => conn.keep_side_message("settings", &setting),
                None => conn.video_settings_out = None,
            },
            vehicle = recv_iap(&mut conn.vehicle_out) => match vehicle {
                Some((lane, message)) => conn.on_vehicle_message(lane, &message),
                None => conn.vehicle_out = None,
            },
            start = recv_started(&mut started) => match start {
                Ok((stream, first_sample)) => conn.audio_started(stream, first_sample),
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => started = None,
            },
        }
    }
    println!("[cpStack] control connection closed {peer}");
    conn.teardown();
    let was_active = conn.live && !stopped;
    let _ = conn.events.send(StackEvent::Closed { was_active });
}

impl<M: Media> Conn<M> {
    fn emit(&self, event: StackEvent) {
        let _ = self.events.send(event);
    }

    fn went_silent(&self) -> bool {
        let Some(answered) = self.timing.as_ref().and_then(TimingSync::last_heard) else {
            return false;
        };
        answered.max(self.last_request).elapsed() > PHONE_SILENT
    }
}

fn mono_ns() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(test)]
pub(crate) mod tests;
