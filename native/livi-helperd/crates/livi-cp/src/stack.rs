use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
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
use crate::iap_tunnel::IapTunnel;
use crate::identity::Identity;
use crate::info::{self, ALT_UUID, InfoConfig, MAIN_UUID};
use crate::keep_alive::KeepAlive;
use crate::media::{AudioCodec, AudioRequest, Media, MicRequest};
use crate::net;
use crate::pair_setup::PairSetup;
use crate::pair_verify::PairVerify;
use crate::pairings::Pairings;
use crate::rtsp::{self, Request, Response};
use crate::timing::ntp64_now;
use crate::timing_sync::TimingSync;

const STREAM_MAIN_SCREEN: u64 = 110;
const STREAM_ALT_SCREEN: u64 = 111;
const STREAM_MAIN_AUDIO: u64 = 100;
const STREAM_ALT_AUDIO: u64 = 101;
const STREAM_MAIN_HIGH_AUDIO: u64 = 102;
/// The generic data stream, the one iAP2 rides.
const STREAM_DATA: u64 = 130;
const IAP_DATASTREAM_UUID: &str = "E9459FD0-BCAD-4C45-820F-1E72447EF2F2";

/// The phone answers the clock exchange every second and asks for feedback every two.
const PHONE_SILENT: Duration = Duration::from_secs(10);
const SILENCE_CHECK: Duration = Duration::from_millis(250);

const CLUSTER_MAP_URL: &str = "maps:/car/instrumentcluster/map";

const AAC_LC_44K_STEREO: u64 = 0x40_0000;
const AAC_LC_48K_STEREO: u64 = 0x80_0000;
const OPUS_16K: u64 = 0x1000_0000;
const OPUS_24K: u64 = 0x2000_0000;
const OPUS_48K: u64 = 0x4000_0000;
const OPUS_MONO: u64 = OPUS_16K | OPUS_24K | OPUS_48K;

const PLIST: &str = "application/x-apple-binary-plist";
const PAIRING: &str = "application/pairing+tlv8";
const OCTET: &str = "application/octet-stream";

/// PCM only comes over the cable.
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

/// The values are the keys of the volume levels.
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
    AudioProfile { kind, label: label.to_string() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCodec {
    H264,
    H265,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StackConfig {
    pub info: InfoConfig,
    /// Sink and source, empty for the system default.
    pub audio_device: String,
    pub audio_input_device: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    /// 0 to 1 across the main screen.
    pub x: f64,
    pub y: f64,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StackCmd {
    Touches(Vec<Touch>),
    Knob { state: KnobState, momentary: bool },
    KnobSelect(bool),
    Media(u8),
    Telephony(u8),
    Siri,
    NightMode(bool),
    LimitedUi(bool),
    Keyframe,
    ClusterActive(bool),
    VideoActive(bool),
    AudioActive(bool),
    StreamVolume { kind: AudioKind, level: f64, ramp_ms: u32 },
    PhoneBtMac(String),
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
    /// Always the last event.
    Closed {
        was_active: bool,
    },
}

pub struct StackCtx<M: Media> {
    pub identity: Identity,
    pub pairings: Pairings,
    pub helper: HelperSock,
    pub media: Arc<M>,
    pub config: watch::Receiver<StackConfig>,
    /// The access point's MAC can change while LIVI runs, and /info must name the current one.
    pub refresh: Option<Arc<dyn Fn() + Send + Sync>>,
    pub debug: bool,
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
    event_listener: Option<TcpListener>,
    event: Option<EventConn>,
    live: bool,
    night_mode: Option<bool>,
    limited_ui: Option<bool>,
    cluster_want: bool,
    video_active: bool,
    audio_active: bool,
    call_active: bool,
    speech_active: bool,
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

async fn recv_iap(out: &mut Option<mpsc::UnboundedReceiver<Vec<u8>>>) -> Option<Vec<u8>> {
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

/// The id salts the stream keys, so its text has to match what the phone derives them with.
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
    if n.is_finite() && n > 0.0 { n as u64 } else { 0 }
}

fn plist_response(body: &Value) -> Response {
    Response {
        headers: vec![("Content-Type".into(), PLIST.into())],
        body: bplist::encode(body),
        ..Default::default()
    }
}

fn status(code: u16) -> Response {
    Response { status: Some(code), ..Default::default() }
}

fn stream_key(shared: &[u8; 32], id: &str, info: &str) -> [u8; 32] {
    hkdf_sha512(shared, format!("DataStream-Salt{id}").as_bytes(), info.as_bytes())
}

pub async fn run<M: Media>(
    stream: TcpStream,
    ctx: Arc<StackCtx<M>>,
    mut cmds: mpsc::UnboundedReceiver<StackCmd>,
    events: mpsc::UnboundedSender<StackEvent>,
) {
    let peer = stream.peer_addr().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
    println!("[cpStack] control connection from {peer}");
    let mut started = Some(ctx.media.audio_started());
    let (mut rd, mut wr) = stream.into_split();
    let mut conn = Conn {
        ctx,
        events,
        peer,
        pair_setup: PairSetup::new(),
        pair_verify: PairVerify::new(),
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
        event_listener: None,
        event: None,
        live: false,
        night_mode: None,
        limited_ui: None,
        cluster_want: false,
        video_active: false,
        audio_active: false,
        call_active: false,
        speech_active: false,
    };
    let mut buf = vec![0u8; 64 * 1024];
    let mut event_buf = vec![0u8; 16 * 1024];
    let mut relay_buf = vec![0u8; 16 * 1024];
    let mut stopped = false;
    let mut silence = tokio::time::interval(SILENCE_CHECK);
    loop {
        tokio::select! {
            read = rd.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    conn.last_request = tokio::time::Instant::now();
                    if conn.on_control_bytes(&buf[..n], &mut wr).await.is_err() {
                        break;
                    }
                }
            },
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

    /// Nothing is judged before the phone first answered the clock exchange.
    fn went_silent(&self) -> bool {
        let Some(answered) = self.timing.as_ref().and_then(TimingSync::last_heard) else {
            return false;
        };
        answered.max(self.last_request).elapsed() > PHONE_SILENT
    }

    async fn on_control_bytes(&mut self, chunk: &[u8], wr: &mut OwnedWriteHalf) -> io::Result<()> {
        if let Some(cipher) = self.cipher.as_mut() {
            self.sealed.extend_from_slice(chunk);
            let (data, rest) = cipher
                .decrypt(&self.sealed)
                .map_err(|_| io::Error::other("control channel decrypt failed"))?;
            self.sealed = rest;
            self.plain.extend(data);
        } else {
            self.plain.extend_from_slice(chunk);
        }
        let (requests, rest) = rtsp::parse(&self.plain);
        self.plain = rest;
        for req in requests {
            let res = self.handle(&req).await;
            let out = rtsp::build_response(&req, res);
            let sealed = match self.cipher.as_mut() {
                Some(cipher) => cipher.encrypt(&out),
                None => out,
            };
            wr.write_all(&sealed).await?;
            // pair-verify M4 goes out in plain text, encryption starts with the next message.
            if self.cipher.is_none()
                && let Some(keys) = self.pair_verify.control_keys()
            {
                self.cipher = Some(ControlCipher::new(keys.read, keys.write));
                println!("[cpStack] control channel encrypted");
            }
        }
        Ok(())
    }

    async fn handle(&mut self, req: &Request) -> Response {
        let path = req.path.to_lowercase();
        let chatty = req.method == "OPTIONS" || path.ends_with("/feedback");
        if !chatty {
            println!("[cpStack] < {} {} (body {}B)", req.method, req.path, req.body.len());
            if self.ctx.debug && !req.body.is_empty() {
                match bplist::decode(&req.body) {
                    Ok(v) => println!("[cpStack]   body: {v:?}"),
                    Err(_) => println!("[cpStack]   body: not a plist ({}B)", req.body.len()),
                }
            }
        }
        match req.method.as_str() {
            "SETUP" => return self.handle_setup(req).await,
            "RECORD" => {
                println!("[cpStack] RECORD (session started)");
                self.live = true;
                // Commands only count once the session runs, older iOS stalls the
                // bring-up on one sent before RECORD.
                if let Some(night) = self.night_mode {
                    self.send_night_mode(night).await;
                }
                if let Some(limited) = self.limited_ui {
                    self.send_limited_ui(limited).await;
                }
                self.emit(StackEvent::Active {
                    ip: net::host_of(&self.peer),
                    controller_id: self.pair_verify.controller_id().map(str::to_string),
                });
                self.open_iap_relay().await;
                return status(200);
            }
            "TEARDOWN" => return self.handle_teardown(req),
            _ => {}
        }
        if path.ends_with("/pair-setup") {
            let body = self.pair_setup.handle(&req.body, &self.ctx.identity, &self.ctx.pairings);
            return Response {
                headers: vec![("Content-Type".into(), PAIRING.into())],
                body,
                ..Default::default()
            };
        }
        if path.ends_with("/pair-verify") {
            let body = self.pair_verify.handle(&req.body, &self.ctx.identity, &self.ctx.pairings);
            return Response {
                headers: vec![("Content-Type".into(), PAIRING.into())],
                body,
                ..Default::default()
            };
        }
        if path.ends_with("/auth-setup") {
            return match auth_setup::handle(&req.body, &self.ctx.helper).await {
                Ok(Some(body)) => Response {
                    headers: vec![("Content-Type".into(), OCTET.into())],
                    body,
                    ..Default::default()
                },
                Ok(None) => status(400),
                Err(e) => {
                    println!("[cpStack] handler error for {} {}: {e}", req.method, req.path);
                    status(500)
                }
            };
        }
        if path.ends_with("/info") {
            if !req.body.is_empty() {
                match bplist::decode(&req.body) {
                    Ok(ask) => println!("[cpStack] /info request from phone: {ask:?}"),
                    Err(_) => {
                        println!("[cpStack] /info request body not a plist ({}B)", req.body.len())
                    }
                }
            }
            if let Some(refresh) = self.ctx.refresh.clone() {
                let _ = tokio::task::spawn_blocking(move || refresh()).await;
            }
            let cfg = self.ctx.config.borrow().info.clone();
            let info = info::build(&cfg);
            let count = |k: &str| info.get(k).and_then(Value::as_array).map_or(0, <[Value]>::len);
            println!(
                "[cpStack] /info audio: disableAudioOutput={} audioFormats={} audioLatencies={}",
                cfg.disable_audio_output,
                count("audioFormats"),
                count("audioLatencies")
            );
            return plist_response(&info);
        }
        if req.method == "POST" && path.ends_with("/command") {
            return self.handle_command(req).await;
        }
        if req.method == "POST" && path.ends_with("/feedback") {
            return self.build_feedback();
        }
        println!("[cpStack] unhandled request {} {} (ack 200)", req.method, req.path);
        status(200)
    }

    async fn handle_setup(&mut self, req: &Request) -> Response {
        let body = match bplist::decode(&req.body) {
            Ok(v) => v,
            Err(e) => {
                println!("[cpStack] SETUP body is not a plist: {e}");
                return status(400);
            }
        };
        if let Some(streams) = body.get("streams").and_then(Value::as_array) {
            let mut out = Vec::new();
            for sd in streams {
                let kind = int(sd.get("type"));
                match kind {
                    STREAM_MAIN_SCREEN | STREAM_ALT_SCREEN => {
                        let port = self.setup_screen(sd, kind == STREAM_ALT_SCREEN).await;
                        match port {
                            Ok(port) => out.push(dict([
                                ("type", Value::Int(kind)),
                                ("dataPort", Value::Int(u64::from(port))),
                            ])),
                            Err(e) => return self.setup_failed(req, &e),
                        }
                    }
                    STREAM_MAIN_AUDIO | STREAM_ALT_AUDIO | STREAM_MAIN_HIGH_AUDIO => {
                        println!("[cpStack] SETUP audio stream type {kind}");
                        match self.setup_audio(sd, kind).await {
                            Ok(v) => out.push(v),
                            Err(e) => return self.setup_failed(req, &e),
                        }
                    }
                    STREAM_DATA => match self.setup_data_stream(sd) {
                        Ok(Some(v)) => out.push(v),
                        Ok(None) => {}
                        Err(e) => return self.setup_failed(req, &e),
                    },
                    other => println!("[cpStack]   SETUP stream type {other} not handled yet"),
                }
            }
            return plist_response(&dict([("streams", Value::Array(out))]));
        }

        // deviceID is the phone's Bluetooth MAC, macAddress its Wi-Fi MAC.
        let text = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (name, device_id, model) = (text("name"), text("deviceID"), text("model"));
        let wifi_mac = text("macAddress").to_lowercase();
        if !device_id.is_empty() {
            self.device_bt_mac = device_id.clone();
        }
        if !name.is_empty() || !device_id.is_empty() || !wifi_mac.is_empty() {
            self.emit(StackEvent::DeviceInfo { name, device_id, wifi_mac, model });
        }

        let mut timing = match TimingSync::listen() {
            Ok(t) => t,
            Err(e) => return self.setup_failed(req, &e.to_string()),
        };
        let timing_port = timing.port();
        let peer_timing = int(body.get("timingPort"));
        if let Ok(port) = u16::try_from(peer_timing)
            && port > 0
        {
            let mut peer = self.peer;
            peer.set_port(port);
            timing.start(peer);
        }
        self.timing = Some(timing);
        let event_port = match net::tcp_listener() {
            Ok(l) => {
                let port = net::local_port(l.local_addr());
                self.event_listener = Some(l);
                port
            }
            Err(e) => return self.setup_failed(req, &e.to_string()),
        };
        let mut keep_alive_port = 0;
        if truthy(body.get("keepAliveLowPower")) {
            match KeepAlive::listen() {
                Ok(k) => {
                    keep_alive_port = k.port();
                    self.keep_alive = Some(k);
                }
                Err(e) => return self.setup_failed(req, &e.to_string()),
            }
        }
        println!(
            "[cpStack] SETUP session (timingPort={timing_port}, eventPort={event_port}, keepAlivePort={keep_alive_port})"
        );
        let cfg = self.ctx.config.borrow().info.clone();
        let mut features = Vec::new();
        if cfg.hevc {
            features.push(Value::String("hevc".into()));
        }
        features.push(Value::String("iAPChannel".into()));
        features.push(Value::String("viewAreas".into()));
        if cfg.cluster.is_some() {
            features.push(Value::String("altScreen".into()));
        }
        let mut resp = vec![
            ("timingPort".to_string(), Value::Int(u64::from(timing_port))),
            ("eventPort".to_string(), Value::Int(u64::from(event_port))),
        ];
        if keep_alive_port > 0 {
            resp.push(("keepAlivePort".to_string(), Value::Int(u64::from(keep_alive_port))));
        }
        resp.push(("enabledFeatures".to_string(), Value::Array(features)));
        plist_response(&Value::Dict(resp))
    }

    fn setup_failed(&self, req: &Request, e: &str) -> Response {
        println!("[cpStack] handler error for {} {}: {e}", req.method, req.path);
        status(500)
    }

    async fn setup_screen(&mut self, sd: &Value, cluster: bool) -> Result<u16, String> {
        let shared =
            self.pair_verify.shared_secret().ok_or("screen SETUP arrived before pair-verify")?;
        let id = id_text(sd.get("streamConnectionID"));
        let key = stream_key(&shared, &id, "DataStream-Output-Encryption-Key");
        let codec =
            if self.ctx.config.borrow().info.hevc { ScreenCodec::H265 } else { ScreenCodec::H264 };
        let (port, receiver) = self.ctx.media.open_screen(cluster, key).await;
        if self.video_active {
            self.ctx.media.set_screen_active(receiver, true);
        }
        if cluster {
            self.cluster_screen = Some(receiver);
            if !self.cluster_codec_emitted {
                self.cluster_codec_emitted = true;
                self.emit(StackEvent::VideoCodec { cluster: true, codec });
            }
        } else {
            self.screen = Some(receiver);
            if !self.codec_emitted {
                self.codec_emitted = true;
                self.emit(StackEvent::VideoCodec { cluster: false, codec });
            }
            if !self.main_stream_ready {
                self.main_stream_ready = true;
                if self.cluster_want {
                    self.activate_cluster_stream().await;
                }
            }
            self.emit(StackEvent::MainScreenReady);
        }
        println!(
            "[cpStack] SETUP screen NATIVE ({}, dataPort={port}, id={id}, codec={codec:?})",
            if cluster { "cluster" } else { "main" }
        );
        Ok(port)
    }

    async fn setup_audio(&mut self, sd: &Value, kind: u64) -> Result<Value, String> {
        let shared =
            self.pair_verify.shared_secret().ok_or("audio SETUP arrived before pair-verify")?;
        let connection_id = sd.get("streamConnectionID").cloned().unwrap_or(Value::Int(0));
        let id = id_text(sd.get("streamConnectionID"));
        let latency_ms = int(sd.get("audioLatencyMs")) as u32;
        let key = stream_key(&shared, &id, "DataStream-Output-Encryption-Key");
        let audio_type = sd.get("audioType").map_or_else(|| "media".to_string(), text);
        let profile = audio_profile(&audio_type);
        let fmt = int(sd.get("audioFormat"));
        let is_aac = fmt & (AAC_LC_44K_STEREO | AAC_LC_48K_STEREO) != 0;
        let is_opus = fmt & OPUS_MONO != 0;
        let (sample_rate, channels) = if is_opus {
            (48000, 1)
        } else if is_aac {
            (if fmt & AAC_LC_48K_STEREO != 0 { 48000 } else { 44100 }, 2)
        } else {
            pcm_format(fmt).unwrap_or((44100, 2))
        };
        let codec = if is_aac {
            AudioCodec::AacLc
        } else if is_opus {
            AudioCodec::Opus
        } else {
            AudioCodec::Pcm
        };
        let (device, input_device) = {
            let cfg = self.ctx.config.borrow();
            (cfg.audio_device.clone(), cfg.audio_input_device.clone())
        };
        let request = AudioRequest {
            codec,
            payload_type: kind as u8,
            clock_rate: if is_opus { 48000 } else { sample_rate },
            channels,
            latency_ms: if latency_ms > 0 { latency_ms } else { 1000 },
            realtime: profile.kind != AudioKind::Media,
            device,
        };
        let opened = self.ctx.media.open_audio(key, request).await;
        if self.audio_active {
            self.ctx.media.set_audio_active(opened.id, true);
        }

        // MainAudio goes both ways: a port in the phone's request asks for the
        // microphone, keyed with the mirror of the output key. Over Wi-Fi the
        // phone picks OPUS for it at its own rate.
        let phone_mic_port = if kind == STREAM_MAIN_AUDIO { int(sd.get("dataPort")) } else { 0 };
        let opus_rate = if fmt & OPUS_48K != 0 {
            48000
        } else if fmt & OPUS_24K != 0 {
            24000
        } else if fmt & OPUS_16K != 0 {
            16000
        } else {
            24000
        };
        let mic_rate = if is_opus { opus_rate } else { sample_rate };
        let mic_channels = if is_opus { 1 } else { channels };
        let frames = int(sd.get("framesPerPacket"));
        let frame_ms = if frames > 0 {
            (frames as f64 / f64::from(mic_rate) * 1000.0 + 0.5).floor() as u32
        } else {
            20
        };
        let bitrate = if mic_rate <= 24000 {
            48000
        } else if mic_rate <= 32000 {
            64000
        } else {
            96000
        };
        let mic_plan = u16::try_from(phone_mic_port).ok().filter(|p| *p > 0).map(|port| {
            (
                stream_key(&shared, &id, "DataStream-Input-Encryption-Key"),
                MicRequest {
                    opus: is_opus,
                    payload_type: kind as u8,
                    sample_rate: mic_rate,
                    channels: mic_channels,
                    bitrate,
                    frame_ms,
                    port,
                    phone: net::host_of(&self.peer),
                    device: input_device,
                },
            )
        });
        println!(
            "[cpStack] SETUP audio (type {kind}, audioType={audio_type}, format=0x{fmt:x}, codec={codec:?}, audioLatencyMs={latency_ms}, dataPort={}, controlPort={}, id={id})",
            opened.data_port, opened.control_port
        );
        self.audio.push(AudioStreamState {
            stream_type: kind,
            sample_rate,
            connection_id: connection_id.clone(),
            host_stream: opened.id,
            mic: None,
            origin: None,
            playout_latency_ms: latency_ms,
            profile,
            mic_plan,
        });
        // The phone matches our answer to its request by the echoed id.
        Ok(dict([
            ("type", Value::Int(kind)),
            ("dataPort", Value::Int(u64::from(opened.data_port))),
            ("controlPort", Value::Int(u64::from(opened.control_port))),
            ("streamConnectionID", connection_id),
        ]))
    }

    fn setup_data_stream(&mut self, sd: &Value) -> Result<Option<Value>, String> {
        let uuid = sd.get("clientTypeUUID").and_then(Value::as_str).unwrap_or("").to_uppercase();
        if uuid != IAP_DATASTREAM_UUID {
            println!(
                "[cpStack]   DataStream {} not handled yet",
                if uuid.is_empty() { "(no uuid)" } else { &uuid }
            );
            return Ok(None);
        }
        let shared = self
            .pair_verify
            .shared_secret()
            .ok_or("iAP DataStream SETUP arrived before pair-verify")?;
        let seed = id_text(sd.get("seed"));
        let (tx, rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&shared, &seed, tx).map_err(|e| e.to_string())?;
        let port = tunnel.port();
        self.iap_tunnel = Some(tunnel);
        self.iap_out = Some(rx);
        println!("[cpStack] SETUP iAP tunnel (type 130, dataPort={port}, seed={seed})");
        Ok(Some(dict([
            ("type", Value::Int(STREAM_DATA)),
            ("streamID", Value::Int(1)),
            ("dataPort", Value::Int(u64::from(port))),
        ])))
    }

    async fn open_iap_relay(&mut self) {
        if self.iap_relay.is_some() {
            return;
        }
        let mac = if self.device_bt_mac.is_empty() {
            self.phone_bt_mac.clone()
        } else {
            self.device_bt_mac.clone()
        };
        let cid = self.pair_verify.controller_id().unwrap_or("").to_string();
        println!(
            "[cpStack] opening iAP relay (cid={cid}, btMac={})",
            if mac.is_empty() { "unknown" } else { &mac }
        );
        match self.ctx.helper.open_tunnel(&cid, &mac).await {
            Ok(sock) => self.iap_relay = Some(sock.into_split()),
            Err(e) => println!("[cpStack] iAP relay error: {e}"),
        }
    }

    async fn relay_iap(&mut self, iap: &[u8]) {
        if let Some((_, wr)) = self.iap_relay.as_mut()
            && wr.write_all(iap).await.is_err()
        {
            self.iap_relay = None;
        }
    }

    fn handle_teardown(&mut self, req: &Request) -> Response {
        let types: Vec<u64> = bplist::decode(&req.body)
            .ok()
            .and_then(|b| {
                b.get("streams")
                    .and_then(Value::as_array)
                    .map(|s| s.iter().map(|s| int(s.get("type"))).collect())
            })
            .unwrap_or_default();
        if types.is_empty() {
            println!("[cpStack] TEARDOWN (session)");
            self.teardown();
            return status(200);
        }
        let list: Vec<String> = types.iter().map(u64::to_string).collect();
        println!("[cpStack] TEARDOWN streams {}", list.join(","));
        for kind in types {
            if kind == STREAM_MAIN_SCREEN || kind == STREAM_ALT_SCREEN {
                self.close_screen(kind == STREAM_ALT_SCREEN);
                continue;
            }
            if let Some(i) = self.audio.iter().position(|a| a.stream_type == kind) {
                let a = self.audio.remove(i);
                self.close_audio(a);
            }
        }
        status(200)
    }

    fn close_screen(&mut self, cluster: bool) {
        let slot = if cluster { &mut self.cluster_screen } else { &mut self.screen };
        if let Some(receiver) = slot.take() {
            self.ctx.media.close_screen(receiver);
        }
    }

    fn close_audio(&mut self, a: AudioStreamState) {
        self.ctx.media.close_audio(a.host_stream);
        self.emit(StackEvent::AudioActive { profile: a.profile.clone(), active: false });
        if a.profile.kind == AudioKind::Call {
            self.call_active = false;
        }
        if let Some(mic) = a.mic {
            self.ctx.media.close_mic(mic);
            self.emit(StackEvent::MicActive { active: false, rate: 0, channels: 0 });
        }
    }

    fn teardown(&mut self) {
        self.timing = None;
        self.keep_alive = None;
        self.close_screen(false);
        self.close_screen(true);
        for a in std::mem::take(&mut self.audio) {
            self.close_audio(a);
        }
        self.iap_tunnel = None;
        self.iap_out = None;
        self.iap_relay = None;
        self.event_listener = None;
    }

    fn audio_started(&mut self, stream: u32, first_sample: u32) {
        let Some(a) = self.audio.iter_mut().find(|a| a.host_stream == stream) else { return };
        a.origin = Some((first_sample, Instant::now()));
        let profile = a.profile.clone();
        let mut mic_on = None;
        if a.mic.is_none()
            && let Some((key, req)) = a.mic_plan.clone()
        {
            let (rate, channels) = (req.sample_rate, req.channels);
            a.mic = Some(self.ctx.media.open_mic(key, req));
            mic_on = Some((rate, channels));
        }
        self.emit(StackEvent::AudioActive { profile, active: true });
        if let Some((rate, channels)) = mic_on {
            self.emit(StackEvent::MicActive { active: true, rate, channels });
        }
    }

    /// The buffered media stream is clock driven, an empty answer makes the phone drop it every
    /// few seconds.
    fn build_feedback(&self) -> Response {
        if self.audio.is_empty() {
            return status(200);
        }
        let streams = self
            .audio
            .iter()
            .map(|a| {
                let mut s = vec![
                    ("type".to_string(), Value::Int(a.stream_type)),
                    ("sampleRate".to_string(), Value::Int(u64::from(a.sample_rate))),
                ];
                if let Some((first, at)) = a.origin {
                    let lag = f64::from(a.playout_latency_ms) / 1000.0;
                    let elapsed = (at.elapsed().as_secs_f64() - lag).max(0.0);
                    let play = (i64::from(first)
                        + (elapsed * f64::from(a.sample_rate) + 0.5).floor() as i64)
                        as u32;
                    let ntp = self.timing.as_ref().map_or_else(ntp64_now, TimingSync::synced_ntp);
                    s.push(("streamConnectionID".to_string(), a.connection_id.clone()));
                    s.push(("timestamp".to_string(), Value::Int(ntp)));
                    s.push(("timestampRawNs".to_string(), Value::Int(mono_ns())));
                    s.push(("sampleTime".to_string(), Value::Int(u64::from(play))));
                }
                Value::Dict(s)
            })
            .collect();
        plist_response(&dict([("streams", Value::Array(streams))]))
    }

    async fn handle_command(&mut self, req: &Request) -> Response {
        let body = bplist::decode(&req.body).unwrap_or(Value::Dict(Vec::new()));
        let kind = body.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let params = body.get("params").cloned().unwrap_or(Value::Dict(Vec::new()));
        match kind.as_str() {
            // The car button in the dock asks for the head unit's own UI.
            "requestUI" => {
                println!("[cpStack] requestUI → host UI requested");
                self.emit(StackEvent::HostUiRequested);
            }
            "disableBluetooth" => {
                let device = params.get("deviceID").and_then(Value::as_str).unwrap_or("");
                println!("[cpStack] disableBluetooth (deviceID={device}) — disconnecting BT");
                self.emit(StackEvent::DisableBluetooth(device.to_string()));
            }
            "modesChanged" => {
                self.modes_changed(&params);
                if self.ctx.debug {
                    println!("[cpStack] modesChanged {params:?}");
                }
            }
            "suggestUI" => {
                let urls = params.get("urls").and_then(Value::as_array).map_or(0, <[Value]>::len);
                println!("[cpStack] suggestUI ({urls} urls, not shown)");
            }
            "iAPSendMessage" => {
                if let Some(data) = params.get("data").and_then(Value::as_data) {
                    let data = data.to_vec();
                    self.relay_iap(&data).await;
                }
            }
            "duckAudio" => {
                let duration = number(params.get("durationMs"));
                let db = number(params.get("volume"));
                let level = 10f64.powf(db / 20.0).clamp(0.0, 1.0);
                println!(
                    "[cpStack] duckAudio volume={db}dB (level={level:.3}) durationMs={duration}"
                );
                self.emit(StackEvent::Duck { level, duration_ms: duration.max(0.0) as u32 });
            }
            "unduckAudio" => {
                let duration = number(params.get("durationMs"));
                println!("[cpStack] unduckAudio durationMs={duration}");
                self.emit(StackEvent::Duck { level: 1.0, duration_ms: duration.max(0.0) as u32 });
            }
            "" => {}
            other => println!("[cpStack] unhandled command '{other}' (ack 200) {body:?}"),
        }
        status(200)
    }

    /// appStateID 1 is Siri, speechMode 1 Recognizing and 2 Speaking. appStateID 2 with entity 1
    /// is a call, which counts as no Siri.
    fn modes_changed(&mut self, params: &Value) {
        let Some(states) = params.get("appStates").and_then(Value::as_array) else { return };
        let mut in_call = self.call_active;
        for s in states {
            if int(s.get("appStateID")) == 2 && s.get("entity").is_some() {
                in_call = int(s.get("entity")) == 1;
            }
        }
        self.call_active = in_call;
        let mut active = self.speech_active;
        for s in states {
            if int(s.get("appStateID")) != 1 {
                continue;
            }
            let Some(mode) = s.get("speechMode") else { continue };
            let mode = number(Some(mode));
            active = !in_call && (mode == 1.0 || mode == 2.0);
        }
        if in_call {
            active = false;
        }
        if active != self.speech_active {
            self.speech_active = active;
            println!("[cpStack] Siri speech {}", if active { "active" } else { "done" });
            self.emit(StackEvent::SpeechActive(active));
        }
    }

    fn on_event_connection(&mut self, sock: TcpStream) {
        println!("[cpStack] event channel connected");
        // Encrypted from the first byte with the Events keys, not swapped: we write
        // with Write and read with Read.
        let Some(shared) = self.pair_verify.shared_secret() else { return };
        let write = hkdf_sha512(&shared, b"Events-Salt", b"Events-Write-Encryption-Key");
        let read = hkdf_sha512(&shared, b"Events-Salt", b"Events-Read-Encryption-Key");
        let (rd, wr) = sock.into_split();
        self.event = Some(EventConn {
            read: rd,
            write: wr,
            cipher: ControlCipher::new(read, write),
            sealed: Vec::new(),
            plain: Vec::new(),
            cseq: 0,
        });
    }

    /// The phone also sends requests of its own here, and each needs an answer.
    async fn on_event_bytes(&mut self, chunk: &[u8]) {
        let Some(ev) = self.event.as_mut() else { return };
        ev.sealed.extend_from_slice(chunk);
        match ev.cipher.decrypt(&ev.sealed) {
            Ok((data, rest)) => {
                ev.sealed = rest;
                ev.plain.extend(data);
            }
            Err(_) => {
                println!("[cpStack] event channel decrypt failed");
                return;
            }
        }
        let (messages, rest) = rtsp::parse(&ev.plain);
        ev.plain = rest;
        for msg in messages {
            if msg.method.starts_with("RTSP/") || msg.method.starts_with("HTTP/") {
                if msg.path != "200" {
                    println!("[cpStack] event response {} {}", msg.path, msg.protocol);
                }
                continue;
            }
            let decoded = if msg.body.is_empty() {
                String::new()
            } else {
                match bplist::decode(&msg.body) {
                    Ok(v) => format!(" {v:?}"),
                    Err(_) => format!(" ({}B non-plist body)", msg.body.len()),
                }
            };
            println!("[cpStack] event < {} {}{decoded}", msg.method, msg.path);
            let answer = rtsp::build_response(&msg, status(200));
            let sealed = ev.cipher.encrypt(&answer);
            if ev.write.write_all(&sealed).await.is_err() {
                return;
            }
        }
    }

    async fn send_event_command(&mut self, body: &Value) {
        let Some(ev) = self.event.as_mut() else { return };
        ev.cseq += 1;
        let payload = bplist::encode(body);
        let mut msg = format!(
            "POST /command RTSP/1.0\r\nContent-Type: {PLIST}\r\nContent-Length: {}\r\nCSeq: {}\r\n\r\n",
            payload.len(),
            ev.cseq
        )
        .into_bytes();
        msg.extend_from_slice(&payload);
        let sealed = ev.cipher.encrypt(&msg);
        let _ = ev.write.write_all(&sealed).await;
    }

    async fn send_hid(&mut self, uid: u32, report: &[u8]) {
        self.send_event_command(&dict([
            ("type", Value::String("hidSendReport".into())),
            ("uuid", Value::String(format!("{uid:x}"))),
            ("hidReport", Value::Data(report.to_vec())),
        ]))
        .await;
    }

    async fn send_night_mode(&mut self, night: bool) {
        self.send_event_command(&dict([
            ("type", Value::String("setNightMode".into())),
            ("params", dict([("nightMode", Value::Bool(night))])),
        ]))
        .await;
    }

    async fn send_limited_ui(&mut self, limited: bool) {
        self.send_event_command(&dict([
            ("type", Value::String("setLimitedUI".into())),
            ("params", dict([("limitedUI", Value::Bool(limited))])),
        ]))
        .await;
    }

    async fn force_keyframe(&mut self, uuid: &str) {
        self.send_event_command(&dict([
            ("type", Value::String("forceKeyFrame".into())),
            ("params", dict([("uuid", Value::String(uuid.into()))])),
        ]))
        .await;
    }

    async fn activate_cluster_stream(&mut self) {
        self.send_event_command(&dict([
            ("type", Value::String("showUI".into())),
            (
                "params",
                dict([
                    ("uuid", Value::String(ALT_UUID.into())),
                    ("url", Value::String(CLUSTER_MAP_URL.into())),
                ]),
            ),
        ]))
        .await;
        self.force_keyframe(ALT_UUID).await;
    }

    async fn on_cmd(&mut self, cmd: StackCmd) {
        match cmd {
            StackCmd::Touches(touches) => {
                if self.event.is_none() {
                    return;
                }
                let (w, h) = {
                    let cfg = self.ctx.config.borrow();
                    (cfg.info.main.width_pixels as f64, cfg.info.main.height_pixels as f64)
                };
                let contacts: Vec<Contact> = touches
                    .iter()
                    .map(|t| Contact { x: t.x * w, y: t.y * h, down: t.down })
                    .collect();
                let report = hid::touch_report(&contacts);
                self.send_hid(hid::TOUCH_HID_UID, &report).await;
            }
            StackCmd::Knob { state, momentary } => {
                self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(state)).await;
                if momentary {
                    self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(KnobState::default())).await;
                }
            }
            StackCmd::KnobSelect(down) => {
                let state = KnobState { select: down, ..Default::default() };
                self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(state)).await;
            }
            StackCmd::Media(index) => {
                self.send_hid(hid::MEDIA_HID_UID, &hid::media_report(index)).await;
                self.send_hid(hid::MEDIA_HID_UID, &hid::media_report(0)).await;
            }
            StackCmd::Telephony(index) => {
                self.send_hid(hid::TELEPHONY_HID_UID, &hid::telephony_report(index)).await;
                self.send_hid(hid::TELEPHONY_HID_UID, &hid::telephony_report(0)).await;
            }
            // siriAction 2 is buttonDown, 3 buttonUp.
            StackCmd::Siri => {
                if self.event.is_none() {
                    println!("[cpStack] invokeSiri: no active event connection, ignoring");
                    return;
                }
                println!("[cpStack] invokeSiri: requestSiri buttonDown+buttonUp (click)");
                for action in [2, 3] {
                    self.send_event_command(&dict([
                        ("type", Value::String("requestSiri".into())),
                        ("params", dict([("siriAction", Value::Int(action))])),
                    ]))
                    .await;
                }
            }
            StackCmd::NightMode(night) => {
                self.night_mode = Some(night);
                self.send_night_mode(night).await;
            }
            StackCmd::LimitedUi(limited) => {
                self.limited_ui = Some(limited);
                self.send_limited_ui(limited).await;
            }
            StackCmd::Keyframe => {
                if self.event.is_none() || !self.main_stream_ready {
                    println!("[cpStack] forceMainKeyframe skipped (stream not ready)");
                } else {
                    println!("[cpStack] forceKeyFrame -> main");
                    self.force_keyframe(MAIN_UUID).await;
                }
                if self.event.is_none() || !self.main_stream_ready || !self.cluster_want {
                    println!(
                        "[cpStack] forceClusterKeyframe skipped (ready={} wantActive={})",
                        self.event.is_some() && self.main_stream_ready,
                        self.cluster_want
                    );
                } else {
                    println!("[cpStack] forceKeyFrame -> cluster");
                    self.force_keyframe(ALT_UUID).await;
                }
            }
            StackCmd::ClusterActive(active) => {
                if self.ctx.config.borrow().info.cluster.is_none() {
                    return;
                }
                self.cluster_want = active;
                if self.event.is_none() || !self.main_stream_ready {
                    return;
                }
                if active {
                    self.activate_cluster_stream().await;
                } else {
                    self.send_event_command(&dict([
                        ("type", Value::String("stopUI".into())),
                        ("params", dict([("uuid", Value::String(ALT_UUID.into()))])),
                    ]))
                    .await;
                }
            }
            StackCmd::VideoActive(active) => {
                self.video_active = active;
                for receiver in [self.screen, self.cluster_screen].into_iter().flatten() {
                    self.ctx.media.set_screen_active(receiver, active);
                }
            }
            StackCmd::AudioActive(active) => {
                self.audio_active = active;
                for a in &self.audio {
                    self.ctx.media.set_audio_active(a.host_stream, active);
                }
            }
            StackCmd::StreamVolume { kind, level, ramp_ms } => {
                for a in self.audio.iter().filter(|a| a.profile.kind == kind) {
                    self.ctx.media.set_audio_volume(a.host_stream, level, ramp_ms);
                }
            }
            StackCmd::PhoneBtMac(mac) => self.phone_bt_mac = mac,
            StackCmd::Stop => {}
        }
    }
}

fn mono_ns() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::crypto::{
        X25519, chacha_seal, ed25519_public, ed25519_sign, nonce_label, random_bytes,
    };
    use crate::identity::load_or_create;
    use crate::media::AudioStream;
    use crate::pair_setup::tlv;
    use crate::pairings::tests::TempDir;
    use crate::tlv8;

    pub(crate) struct FakeMedia {
        calls: Mutex<Vec<String>>,
        pub(crate) started: broadcast::Sender<(u32, u32)>,
    }

    impl FakeMedia {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self { calls: Mutex::default(), started: broadcast::channel(8).0 })
        }

        fn log(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }

        pub(crate) fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Media for FakeMedia {
        fn open_screen(
            &self,
            cluster: bool,
            _key: [u8; 32],
        ) -> impl Future<Output = (u16, u32)> + Send {
            self.log(format!("open_screen {cluster}"));
            async { (5000, 7) }
        }
        fn set_screen_active(&self, receiver: u32, active: bool) {
            self.log(format!("screen_active {receiver} {active}"));
        }
        fn close_screen(&self, receiver: u32) {
            self.log(format!("close_screen {receiver}"));
        }
        fn open_audio(
            &self,
            _key: [u8; 32],
            req: AudioRequest,
        ) -> impl Future<Output = AudioStream> + Send {
            self.log(format!(
                "open_audio {:?} {} {} {} {} {}",
                req.codec, req.clock_rate, req.channels, req.latency_ms, req.realtime, req.device
            ));
            async { AudioStream { id: 9, data_port: 6001, control_port: 6002 } }
        }
        fn set_audio_active(&self, stream: u32, active: bool) {
            self.log(format!("audio_active {stream} {active}"));
        }
        fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32) {
            self.log(format!("volume {stream} {level} {ramp_ms}"));
        }
        fn close_audio(&self, stream: u32) {
            self.log(format!("close_audio {stream}"));
        }
        fn open_mic(&self, _key: [u8; 32], req: MicRequest) -> u32 {
            self.log(format!(
                "open_mic {} {} {} {} {} {}",
                req.opus, req.sample_rate, req.frame_ms, req.port, req.phone, req.device
            ));
            11
        }
        fn close_mic(&self, id: u32) {
            self.log(format!("close_mic {id}"));
        }
        fn audio_started(&self) -> broadcast::Receiver<(u32, u32)> {
            self.started.subscribe()
        }
    }

    pub(crate) struct Phone {
        pub(crate) sock: TcpStream,
        cipher: Option<ControlCipher>,
        buf: Vec<u8>,
        cseq: u32,
        pub(crate) shared: [u8; 32],
    }

    impl Phone {
        pub(crate) fn new(sock: TcpStream) -> Self {
            Self { sock, cipher: None, buf: Vec::new(), cseq: 0, shared: [0; 32] }
        }

        pub(crate) async fn request(
            &mut self,
            method: &str,
            path: &str,
            body: &[u8],
        ) -> (String, Vec<u8>) {
            self.cseq += 1;
            let mut msg = format!(
                "{method} {path} RTSP/1.0\r\nCSeq: {}\r\nContent-Length: {}\r\n\r\n",
                self.cseq,
                body.len()
            )
            .into_bytes();
            msg.extend_from_slice(body);
            let out = match self.cipher.as_mut() {
                Some(c) => c.encrypt(&msg),
                None => msg,
            };
            self.sock.write_all(&out).await.unwrap();
            let mut sealed = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let n = tokio::time::timeout(Duration::from_secs(5), self.sock.read(&mut chunk))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(n > 0, "connection closed");
                match self.cipher.as_mut() {
                    Some(c) => {
                        sealed.extend_from_slice(&chunk[..n]);
                        let (plain, rest) = c.decrypt(&sealed).unwrap();
                        sealed = rest;
                        self.buf.extend(plain);
                    }
                    None => self.buf.extend_from_slice(&chunk[..n]),
                }
                let (mut answers, rest) = rtsp::parse(&self.buf);
                if let Some(answer) = answers.pop() {
                    self.buf = rest;
                    return (answer.path, answer.body);
                }
            }
        }

        pub(crate) async fn plist(&mut self, method: &str, path: &str, body: &Value) -> Value {
            let (code, body) = self.request(method, path, &bplist::encode(body)).await;
            assert_eq!(code, "200");
            if body.is_empty() { Value::Dict(Vec::new()) } else { bplist::decode(&body).unwrap() }
        }

        pub(crate) async fn verify(&mut self, secret: &[u8; 32], id: &[u8]) {
            let eph = X25519::generate();
            let eph_public = eph.public;
            let m1 = tlv8::encode(&[(tlv::STATE, &[1]), (tlv::PUBLIC_KEY, &eph_public)]);
            let (_, m2) = self.request("POST", "/pair-verify", &m1).await;
            let m2 = tlv8::decode(&m2);
            let acc: [u8; 32] = m2[&tlv::PUBLIC_KEY].as_slice().try_into().unwrap();
            let shared = eph.shared(&acc).unwrap();
            let enc =
                hkdf_sha512(&shared, b"Pair-Verify-Encrypt-Salt", b"Pair-Verify-Encrypt-Info");
            let sig = ed25519_sign(secret, &[eph_public.as_slice(), id, &acc].concat());
            let sub = tlv8::encode(&[(tlv::IDENTIFIER, id), (tlv::SIGNATURE, &sig)]);
            let sealed = chacha_seal(&enc, &nonce_label("PV-Msg03"), &sub, &[]);
            let m3 = tlv8::encode(&[(tlv::STATE, &[3]), (tlv::ENCRYPTED_DATA, &sealed)]);
            self.request("POST", "/pair-verify", &m3).await;
            let write = hkdf_sha512(&shared, b"Control-Salt", b"Control-Write-Encryption-Key");
            let read = hkdf_sha512(&shared, b"Control-Salt", b"Control-Read-Encryption-Key");
            self.cipher = Some(ControlCipher::new(read, write));
            self.shared = shared;
        }
    }

    struct Rig {
        _dir: TempDir,
        media: Arc<FakeMedia>,
        phone: Phone,
        cmds: mpsc::UnboundedSender<StackCmd>,
        events: mpsc::UnboundedReceiver<StackEvent>,
        helper_lines: Arc<Mutex<Vec<String>>>,
        secret: [u8; 32],
    }

    impl Rig {
        async fn new() -> Self {
            let dir = TempDir::new();
            let identity = load_or_create(&dir.0.join("identity.json"));
            let pairings = Pairings::new(dir.0.join("pairings.json"));
            let secret: [u8; 32] = random_bytes();
            pairings.save("phone-1", &ed25519_public(&secret));
            let helper_path = dir.0.join("cp.sock");
            let helper_lines = crate::helper_sock::tests::serve(&helper_path, |_| None);
            let mut info = crate::info::tests::sample();
            info.hevc = false;
            let (_tx, config) = watch::channel(StackConfig {
                info,
                audio_device: "out".into(),
                audio_input_device: "in".into(),
            });
            std::mem::forget(_tx);
            let media = FakeMedia::new();
            let ctx = Arc::new(StackCtx {
                identity,
                pairings,
                helper: HelperSock::new(&helper_path),
                media: media.clone(),
                config,
                refresh: Some(Arc::new(|| println!("[test] refreshed"))),
                debug: true,
            });
            let listener = net::tcp_listener().unwrap();
            let port = net::local_port(listener.local_addr());
            let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let (accepted, _) = listener.accept().await.unwrap();
            let (cmds, cmd_rx) = mpsc::unbounded_channel();
            let (ev_tx, events) = mpsc::unbounded_channel();
            tokio::spawn(run(accepted, ctx, cmd_rx, ev_tx));
            let phone = Phone { sock, cipher: None, buf: Vec::new(), cseq: 0, shared: [0; 32] };
            Self { _dir: dir, media, phone, cmds, events, helper_lines, secret }
        }

        async fn event(&mut self) -> StackEvent {
            tokio::time::timeout(Duration::from_secs(5), self.events.recv()).await.unwrap().unwrap()
        }
    }

    fn int_of(v: &Value, k: &str) -> u64 {
        v.get(k).and_then(Value::as_int).unwrap()
    }

    #[test]
    fn audio_types_map_onto_kinds() {
        assert_eq!(audio_profile("telephony").kind, AudioKind::Call);
        assert_eq!(audio_profile("speechRecognition").label, "speech");
        assert_eq!(audio_profile("media").kind, AudioKind::Media);
        assert_eq!(audio_profile("alert").label, "alert");
        assert_eq!(audio_profile("").label, "nav");
        assert_eq!(pcm_format(0x8000), Some((48000, 2)));
        assert_eq!(pcm_format(0x3), None);
    }

    #[test]
    fn plist_values_read_like_js() {
        assert_eq!(id_text(Some(&Value::Int(42))), "42");
        assert_eq!(id_text(Some(&Value::Real(3.0))), "3");
        assert_eq!(id_text(Some(&Value::Real(0.5))), "0.5");
        assert_eq!(id_text(Some(&Value::String("x".into()))), "x");
        assert_eq!(id_text(None), "undefined");
        assert_eq!(text(&Value::Bool(true)), "true");
        assert_eq!(number(Some(&Value::String(" 7 ".into()))), 7.0);
        assert_eq!(number(Some(&Value::Bool(true))), 1.0);
        assert_eq!(number(Some(&Value::Data(vec![]))), 0.0);
        assert_eq!(int(Some(&Value::Real(-3.0))), 0);
        assert!(!truthy(Some(&Value::Int(0))));
        assert!(truthy(Some(&Value::Array(vec![]))));
        assert!(!truthy(Some(&Value::Real(f64::NAN))));
        assert!(!truthy(Some(&Value::String(String::new()))));
        assert!(!truthy(None));
    }

    #[tokio::test]
    async fn plain_requests_before_pairing() {
        let mut rig = Rig::new().await;
        let info = rig.phone.plist("GET", "/info", &dict([("qualifier", Value::Bool(true))])).await;
        assert!(info.get("displays").is_some());
        let (code, _) = rig.phone.request("OPTIONS", "*", b"").await;
        assert_eq!(code, "200");
        let (code, _) = rig.phone.request("GET", "/whatever", b"").await;
        assert_eq!(code, "200");
        let (code, _) = rig.phone.request("SETUP", "/s", b"nope").await;
        assert_eq!(code, "400");
        let screen = dict([("streams", Value::Array(vec![dict([("type", Value::Int(110))])]))]);
        let (code, _) = rig.phone.request("SETUP", "/s", &bplist::encode(&screen)).await;
        assert_eq!(code, "500");
        let (code, _) = rig.phone.request("POST", "/auth-setup", b"short").await;
        assert_eq!(code, "400");
        let (code, _) = rig.phone.request("TEARDOWN", "/s", b"").await;
        assert_eq!(code, "200");
        let (code, _) = rig.phone.request("POST", "/feedback", b"").await;
        assert_eq!(code, "200");
        let (code, setup) =
            rig.phone.request("POST", "/pair-setup", &tlv8::encode(&[(tlv::STATE, &[1])])).await;
        assert_eq!(code, "200");
        assert!(!setup.is_empty());
        rig.phone.sock.shutdown().await.unwrap();
        assert_eq!(rig.event().await, StackEvent::Closed { was_active: false });
    }

    #[tokio::test(start_paused = true)]
    async fn a_phone_that_falls_silent_ends_the_session() {
        let mut rig = Rig::new().await;
        let secret = rig.secret;
        rig.phone.verify(&secret, b"phone-1").await;
        let resp = rig.phone.plist("SETUP", "/s", &dict([("timingPort", Value::Int(9))])).await;

        // Before the phone has answered the clock exchange nothing is judged.
        tokio::time::sleep(PHONE_SILENT * 2).await;
        let (code, _) = rig.phone.request("POST", "/feedback", b"").await;
        assert_eq!(code, "200");

        let clock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let timing_port = int_of(&resp, "timingPort") as u16;
        clock.send_to(&[0u8; crate::timing::PACKET_LEN], ("127.0.0.1", timing_port)).await.unwrap();
        tokio::time::sleep(PHONE_SILENT - Duration::from_secs(1)).await;
        let (code, _) = rig.phone.request("POST", "/feedback", b"").await;
        assert_eq!(code, "200");

        tokio::time::sleep(PHONE_SILENT).await;
        loop {
            if let StackEvent::Closed { .. } = rig.event().await {
                break;
            }
        }
    }

    #[tokio::test]
    async fn a_verified_session_streams_and_takes_input() {
        let mut rig = Rig::new().await;
        let secret = rig.secret;
        rig.phone.verify(&secret, b"phone-1").await;

        let session = dict([
            ("name", Value::String("iPhone".into())),
            ("deviceID", Value::String("AA:BB".into())),
            ("macAddress", Value::String("CC:DD".into())),
            ("model", Value::String("iPhone99".into())),
            ("timingPort", Value::Int(9)),
            ("keepAliveLowPower", Value::Bool(true)),
        ]);
        let resp = rig.phone.plist("SETUP", "/s", &session).await;
        assert_eq!(
            rig.event().await,
            StackEvent::DeviceInfo {
                name: "iPhone".into(),
                device_id: "AA:BB".into(),
                wifi_mac: "cc:dd".into(),
                model: "iPhone99".into(),
            }
        );
        assert!(int_of(&resp, "keepAlivePort") > 0);
        assert_eq!(
            resp.get("enabledFeatures"),
            Some(&Value::Array(vec![
                Value::String("iAPChannel".into()),
                Value::String("viewAreas".into()),
            ]))
        );

        let event_port = int_of(&resp, "eventPort") as u16;
        let mut event = TcpStream::connect(("127.0.0.1", event_port)).await.unwrap();
        let shared = rig.phone.shared;
        let mut event_cipher = ControlCipher::new(
            hkdf_sha512(&shared, b"Events-Salt", b"Events-Write-Encryption-Key"),
            hkdf_sha512(&shared, b"Events-Salt", b"Events-Read-Encryption-Key"),
        );

        rig.cmds.send(StackCmd::VideoActive(true)).unwrap();
        rig.cmds.send(StackCmd::AudioActive(true)).unwrap();
        rig.cmds.send(StackCmd::PhoneBtMac("FF:FF".into())).unwrap();
        let streams = dict([(
            "streams",
            Value::Array(vec![
                dict([("type", Value::Int(110)), ("streamConnectionID", Value::Int(42))]),
                dict([
                    ("type", Value::Int(100)),
                    ("streamConnectionID", Value::Int(43)),
                    ("audioType", Value::String("telephony".into())),
                    ("audioFormat", Value::Int(0x10)),
                    ("dataPort", Value::Int(6000)),
                    ("framesPerPacket", Value::Int(320)),
                ]),
                dict([
                    ("type", Value::Int(130)),
                    ("clientTypeUUID", Value::String("other".into())),
                ]),
                dict([("type", Value::Int(999))]),
            ]),
        )]);
        let resp = rig.phone.plist("SETUP", "/s", &streams).await;
        let out = resp.get("streams").and_then(Value::as_array).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(int_of(&out[0], "dataPort"), 5000);
        assert_eq!(int_of(&out[1], "dataPort"), 6001);
        assert_eq!(int_of(&out[1], "controlPort"), 6002);
        assert_eq!(int_of(&out[1], "streamConnectionID"), 43);
        assert_eq!(
            rig.event().await,
            StackEvent::VideoCodec { cluster: false, codec: ScreenCodec::H264 }
        );
        assert_eq!(rig.event().await, StackEvent::MainScreenReady);

        let (code, _) = rig.phone.request("RECORD", "/s", b"").await;
        assert_eq!(code, "200");
        assert_eq!(
            rig.event().await,
            StackEvent::Active { ip: "127.0.0.1".into(), controller_id: Some("phone-1".into()) }
        );

        rig.media.started.send((99, 1)).unwrap();
        rig.media.started.send((9, 100)).unwrap();
        let call = AudioProfile { kind: AudioKind::Call, label: "telephony".into() };
        assert_eq!(
            rig.event().await,
            StackEvent::AudioActive { profile: call.clone(), active: true }
        );
        assert_eq!(
            rig.event().await,
            StackEvent::MicActive { active: true, rate: 16000, channels: 1 }
        );

        let fb = rig.phone.plist("POST", "/feedback", &dict([])).await;
        let fb = &fb.get("streams").and_then(Value::as_array).unwrap()[0];
        assert_eq!(int_of(fb, "type"), 100);
        assert_eq!(int_of(fb, "sampleRate"), 16000);
        assert!(fb.get("sampleTime").is_some());

        let duck = dict([
            ("type", Value::String("duckAudio".into())),
            ("params", dict([("volume", Value::Int(0)), ("durationMs", Value::Int(500))])),
        ]);
        rig.phone.plist("POST", "/command", &duck).await;
        assert_eq!(rig.event().await, StackEvent::Duck { level: 1.0, duration_ms: 500 });
        let unduck = dict([("type", Value::String("unduckAudio".into()))]);
        rig.phone.plist("POST", "/command", &unduck).await;
        assert_eq!(rig.event().await, StackEvent::Duck { level: 1.0, duration_ms: 0 });
        let siri = dict([
            ("type", Value::String("modesChanged".into())),
            (
                "params",
                dict([(
                    "appStates",
                    Value::Array(vec![dict([
                        ("appStateID", Value::Int(1)),
                        ("speechMode", Value::Int(2)),
                    ])]),
                )]),
            ),
        ]);
        rig.phone.plist("POST", "/command", &siri).await;
        assert_eq!(rig.event().await, StackEvent::SpeechActive(true));
        let call_state = dict([
            ("type", Value::String("modesChanged".into())),
            (
                "params",
                dict([(
                    "appStates",
                    Value::Array(vec![dict([
                        ("appStateID", Value::Int(2)),
                        ("entity", Value::Int(1)),
                    ])]),
                )]),
            ),
        ]);
        rig.phone.plist("POST", "/command", &call_state).await;
        assert_eq!(rig.event().await, StackEvent::SpeechActive(false));
        for cmd in ["requestUI", "disableBluetooth", "suggestUI", "iAPSendMessage", "mystery"] {
            let body = dict([
                ("type", Value::String(cmd.into())),
                (
                    "params",
                    dict([
                        ("deviceID", Value::String("AA:BB".into())),
                        ("data", Value::Data(vec![1])),
                    ]),
                ),
            ]);
            rig.phone.plist("POST", "/command", &body).await;
        }
        assert_eq!(rig.event().await, StackEvent::HostUiRequested);
        assert_eq!(rig.event().await, StackEvent::DisableBluetooth("AA:BB".into()));

        rig.cmds.send(StackCmd::Touches(vec![Touch { x: 0.5, y: 0.5, down: true }])).unwrap();
        rig.cmds.send(StackCmd::Keyframe).unwrap();
        rig.cmds.send(StackCmd::NightMode(true)).unwrap();
        rig.cmds.send(StackCmd::LimitedUi(true)).unwrap();
        rig.cmds.send(StackCmd::Siri).unwrap();
        rig.cmds.send(StackCmd::Media(hid::media_button::NEXT)).unwrap();
        rig.cmds.send(StackCmd::Telephony(hid::telephony_button::DROP)).unwrap();
        rig.cmds
            .send(StackCmd::Knob {
                state: KnobState { x: 1.0, ..Default::default() },
                momentary: true,
            })
            .unwrap();
        rig.cmds.send(StackCmd::KnobSelect(true)).unwrap();
        rig.cmds.send(StackCmd::ClusterActive(true)).unwrap();
        rig.cmds
            .send(StackCmd::StreamVolume { kind: AudioKind::Call, level: 0.5, ramp_ms: 10 })
            .unwrap();
        let mut sealed = Vec::new();
        let mut plain = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        let mut commands = Vec::new();
        while commands.len() < 13 {
            let n = tokio::time::timeout(Duration::from_secs(5), event.read(&mut chunk))
                .await
                .unwrap()
                .unwrap();
            sealed.extend_from_slice(&chunk[..n]);
            let (data, rest) = event_cipher.decrypt(&sealed).unwrap();
            sealed = rest;
            plain.extend(data);
            let (msgs, rest) = rtsp::parse(&plain);
            plain = rest;
            for m in msgs {
                let body = bplist::decode(&m.body).unwrap();
                commands.push(body.get("type").and_then(Value::as_str).unwrap().to_string());
            }
        }
        assert_eq!(
            commands,
            [
                "hidSendReport",
                "forceKeyFrame",
                "setNightMode",
                "setLimitedUI",
                "requestSiri",
                "requestSiri",
                "hidSendReport",
                "hidSendReport",
                "hidSendReport",
                "hidSendReport",
                "hidSendReport",
                "hidSendReport",
                "hidSendReport",
            ]
        );

        let ask =
            event_cipher.encrypt(b"POST /ping RTSP/1.0\r\nCSeq: 1\r\nContent-Length: 0\r\n\r\n");
        event.write_all(&ask).await.unwrap();
        let reply = loop {
            let n = tokio::time::timeout(Duration::from_secs(5), event.read(&mut chunk))
                .await
                .unwrap()
                .unwrap();
            sealed.extend_from_slice(&chunk[..n]);
            let (data, rest) = event_cipher.decrypt(&sealed).unwrap();
            sealed = rest;
            plain.extend(data);
            let (mut msgs, rest) = rtsp::parse(&plain);
            plain = rest;
            if let Some(m) = msgs.pop() {
                break m;
            }
        };
        assert_eq!(reply.path, "200");

        let teardown = dict([(
            "streams",
            Value::Array(vec![
                dict([("type", Value::Int(100))]),
                dict([("type", Value::Int(110))]),
            ]),
        )]);
        rig.phone.plist("TEARDOWN", "/s", &teardown).await;
        assert_eq!(rig.event().await, StackEvent::AudioActive { profile: call, active: false });
        assert_eq!(
            rig.event().await,
            StackEvent::MicActive { active: false, rate: 0, channels: 0 }
        );

        rig.phone.sock.shutdown().await.unwrap();
        assert_eq!(rig.event().await, StackEvent::Closed { was_active: true });
        let calls = rig.media.calls();
        for expected in [
            "open_screen false",
            "screen_active 7 true",
            "open_audio Pcm 16000 1 1000 true out",
            "audio_active 9 true",
            "open_mic false 16000 20 6000 127.0.0.1 in",
            "volume 9 0.5 10",
            "close_audio 9",
            "close_mic 11",
            "close_screen 7",
        ] {
            assert!(calls.iter().any(|c| c == expected), "{expected} missing from {calls:?}");
        }
        assert!(rig.helper_lines.lock().unwrap().iter().any(|l| l == "tunnel phone-1 AA:BB"));
    }

    #[tokio::test]
    async fn stop_ends_the_session_quietly() {
        let mut rig = Rig::new().await;
        rig.cmds.send(StackCmd::Siri).unwrap();
        rig.cmds.send(StackCmd::Stop).unwrap();
        assert_eq!(rig.event().await, StackEvent::Closed { was_active: false });
    }
}
