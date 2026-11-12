// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};

use crate::bplist::{self, Value, dict};
use crate::control_cipher::ControlCipher;
use crate::helper_sock::HelperSock;
use crate::identity::Identity;
use crate::mfi4::session::TunnelError;
use crate::pair_setup::PairSetup;
use crate::pair_verify::PairVerify;
use crate::pairings::Pairings;
use crate::rtsp::{self, Request, Response};
use crate::session_keys;
use crate::timing_sync::TimingSync;
use crate::{auth_setup, mfi4, net, tlv8};

const TAG: &str = "[airplay]";
const TLV_FLAGS: u8 = 0x13;
const FLAG_TRANSIENT: u8 = 0x10;
const STREAM_REALTIME_AUDIO: u64 = 96;
const STREAM_SCREEN: u64 = 110;
const CT_ALAC: u64 = 2;
const ALAC_44K: u64 = 1 << 18;
const ALAC_44K_24: u64 = 1 << 19;
const ALAC_48K: u64 = 1 << 20;
const ALAC_48K_24: u64 = 1 << 21;
const AAC_ELD_44K: u64 = 1 << 24;
const AAC_ELD_48K: u64 = 1 << 25;
const DISPLAY_UUID: &str = "4c495649-4d49-5252-4f52-000000000001";
const CONNECT_WITHIN: Duration = Duration::from_secs(60);
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
const PLIST: &str = "application/x-apple-binary-plist";

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq)]
pub enum AirplayMeta {
    Track {
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
    },
    Artwork(Vec<u8>),
    Progress {
        elapsed_ms: f64,
        duration_ms: f64,
    },
    Playing(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirplayCodec {
    AacEld,
    Alac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirplayFormat {
    pub codec: AirplayCodec,
    pub rate: u32,
    pub bits: u8,
}

impl AirplayFormat {
    fn of(sd: &Value) -> Self {
        let format = |codec, rate, bits| Self { codec, rate, bits };
        match sd.get("audioFormat").and_then(Value::as_int) {
            Some(ALAC_44K) => return format(AirplayCodec::Alac, 44_100, 16),
            Some(ALAC_44K_24) => return format(AirplayCodec::Alac, 44_100, 24),
            Some(ALAC_48K) => return format(AirplayCodec::Alac, 48_000, 16),
            Some(ALAC_48K_24) => return format(AirplayCodec::Alac, 48_000, 24),
            Some(AAC_ELD_44K) => return format(AirplayCodec::AacEld, 44_100, 16),
            Some(AAC_ELD_48K) => return format(AirplayCodec::AacEld, 48_000, 16),
            _ => {}
        }
        let codec = match sd.get("ct").and_then(Value::as_int) {
            Some(CT_ALAC) => AirplayCodec::Alac,
            _ => AirplayCodec::AacEld,
        };
        let rate = sd
            .get("sr")
            .and_then(Value::as_int)
            .and_then(|r| u32::try_from(r).ok())
            .unwrap_or(44_100);
        format(codec, rate, 16)
    }
}

fn audio_formats(rate: u32) -> u64 {
    let base = ALAC_44K | ALAC_44K_24 | AAC_ELD_44K;
    if rate == 48_000 {
        base | ALAC_48K | ALAC_48K_24 | AAC_ELD_48K
    } else {
        base
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AirplaySender {
    pub name: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AirplayCommand {
    Play,
    Pause,
    Next,
    Prev,
    Volume(f64),
    Seek(f64),
}

impl AirplayCommand {
    fn code(self) -> &'static str {
        match self {
            AirplayCommand::Play => "play",
            AirplayCommand::Pause => "paus",
            AirplayCommand::Next => "nitm",
            AirplayCommand::Prev => "pitm",
            AirplayCommand::Volume(_) => "dvlc",
            AirplayCommand::Seek(_) => "seek",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AirplayRemote(mpsc::UnboundedSender<AirplayCommand>);

impl AirplayRemote {
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<AirplayCommand>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self(tx), rx)
    }

    pub fn send(&self, command: AirplayCommand) {
        let _ = self.0.send(command);
    }
}

pub trait AirplayMedia: Send + Sync {
    fn open_screen(&self, key: [u8; 32]) -> Pin<Box<dyn Future<Output = u16> + Send + '_>>;
    fn close_screen(&self);
    fn open_audio(
        &self,
        key: [u8; 32],
        format: AirplayFormat,
    ) -> Pin<Box<dyn Future<Output = (u16, u16)> + Send + '_>>;
    fn close_audio(&self);
    fn started(&self, id: u64, close: Arc<Notify>, peer: AirplaySender, remote: AirplayRemote);
    fn ended(&self, id: u64);
    fn metadata(&self, id: u64, meta: AirplayMeta);
    fn volume(&self, db: f64);
    fn volume_db(&self) -> f64;
}

pub struct AirplayCtx {
    pub media: Arc<dyn AirplayMedia>,
    pub identity: Identity,
    pub pairings: Pairings,
    pub helper: HelperSock,
    pub device_id: String,
    pub name: String,
    pub source_version: String,
    pub features: u64,
    pub txt: Vec<String>,
    pub sample_rate: u32,
}

pub async fn serve(listener: TcpListener, ctx: Arc<AirplayCtx>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(connection(stream, peer, ctx.clone()));
            }
            Err(e) => {
                println!("{TAG} accept failed: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    elapsed_ms: f64,
    duration_ms: f64,
    at: Instant,
    running: bool,
}

impl Clock {
    fn elapsed_at(&self, now: Instant) -> f64 {
        let ran = if self.running {
            now.saturating_duration_since(self.at).as_secs_f64() * 1000.0
        } else {
            0.0
        };
        (self.elapsed_ms + ran).min(self.duration_ms)
    }
}

struct Conn {
    ctx: Arc<AirplayCtx>,
    peer: SocketAddr,
    local: SocketAddr,
    pair_setup: PairSetup,
    pair_verify: PairVerify,
    mfi4: Option<mfi4::Session>,
    cipher: Option<ControlCipher>,
    sealed: Vec<u8>,
    plain: Vec<u8>,
    timing: Option<TimingSync>,
    transient: bool,
    audio: bool,
    id: u64,
    close: Arc<Notify>,
    started: bool,
    sender: AirplaySender,
    remote: AirplayRemote,
    commands: Option<mpsc::UnboundedReceiver<AirplayCommand>>,
    clock: Option<Clock>,
    playing: bool,
    rate: u32,
}

async fn connection(stream: TcpStream, peer: SocketAddr, ctx: Arc<AirplayCtx>) {
    let local = stream.local_addr().unwrap_or(peer);
    let (remote, commands) = AirplayRemote::channel();
    let (mut rd, mut wr) = stream.into_split();
    let mut conn = Conn {
        ctx,
        peer,
        local,
        pair_setup: PairSetup::new(),
        pair_verify: PairVerify::new(),
        mfi4: None,
        cipher: None,
        sealed: Vec::new(),
        plain: Vec::new(),
        timing: None,
        transient: false,
        audio: false,
        id: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
        close: Arc::new(Notify::new()),
        started: false,
        sender: AirplaySender::default(),
        remote,
        commands: Some(commands),
        clock: None,
        playing: false,
        rate: 44_100,
    };
    let close = conn.close.clone();
    let mut buf = vec![0u8; 64 * 1024];
    let mut ticker = tokio::time::interval(PROGRESS_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let read = tokio::select! {
            read = rd.read(&mut buf) => read,
            _ = ticker.tick() => {
                conn.tick();
                continue;
            }
            () = close.notified() => {
                println!("{TAG} {peer}: closed by LIVI");
                break;
            }
        };
        let n = match read {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                println!("{TAG} read from {peer}: {e}");
                break;
            }
        };
        if let Err(e) = conn.on_bytes(&buf[..n], &mut wr).await {
            println!("{TAG} {peer}: {e}");
            break;
        }
    }
    if conn.audio {
        conn.ctx.media.close_audio();
    }
    if conn.started {
        conn.ctx.media.ended(conn.id);
    }
}

impl Conn {
    async fn on_bytes(&mut self, chunk: &[u8], wr: &mut OwnedWriteHalf) -> io::Result<()> {
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
        let (requests, rest) =
            rtsp::parse(&self.plain).map_err(|_| io::Error::other("request too large"))?;
        self.plain = rest;
        for req in requests {
            let res = self.handle(&req).await;
            let out = rtsp::build_response(&req, res);
            let sealed = match self.cipher.as_mut() {
                Some(cipher) => cipher.encrypt(&out),
                None => out,
            };
            wr.write_all(&sealed).await?;
            if self.cipher.is_none()
                && let Some(keys) = self.pair_verify.control_keys()
            {
                self.cipher = Some(ControlCipher::new(keys.read, keys.write));
            }
            if self.cipher.is_none()
                && self.transient
                && req.path.to_lowercase().ends_with("/pair-setup")
                && let Some(k) = self.pair_setup.session_key()
            {
                let (read, write) = session_keys::control(&k);
                self.cipher = Some(ControlCipher::new(read, write));
            }
        }
        Ok(())
    }

    fn tell(&mut self, meta: AirplayMeta) {
        let now = Instant::now();
        match &meta {
            AirplayMeta::Progress {
                elapsed_ms,
                duration_ms,
            } => {
                self.clock = Some(Clock {
                    elapsed_ms: *elapsed_ms,
                    duration_ms: *duration_ms,
                    at: now,
                    running: self.playing,
                });
            }
            AirplayMeta::Playing(playing) => {
                self.playing = *playing;
                if let Some(clock) = self.clock.as_mut() {
                    clock.elapsed_ms = clock.elapsed_at(now);
                    clock.at = now;
                    clock.running = *playing;
                }
            }
            _ => {}
        }
        self.ctx.media.metadata(self.id, meta);
    }

    fn tick(&self) {
        if let Some(clock) = self.clock.filter(|c| c.running) {
            self.ctx.media.metadata(
                self.id,
                AirplayMeta::Progress {
                    elapsed_ms: clock.elapsed_at(Instant::now()),
                    duration_ms: clock.duration_ms,
                },
            );
        }
    }

    async fn handle(&mut self, req: &Request) -> Response {
        let path = req.path.to_lowercase();
        if path.ends_with("/fp-setup") {
            return status(404);
        }
        if path.ends_with("/pair-setup") {
            return self.pair_setup(req).await;
        }
        if path.ends_with("/pair-verify") {
            let body = self
                .pair_verify
                .handle(&req.body, &self.ctx.identity, &self.ctx.pairings);
            return pairing(body);
        }
        if path.ends_with("/auth-setup") {
            return self.auth_setup(req).await;
        }
        if path.ends_with("/info") {
            let asked_txt = bplist::decode(&req.body).ok().is_some_and(|v| {
                v.get("qualifier")
                    .and_then(Value::as_array)
                    .is_some_and(|q| q.iter().any(|s| s.as_str() == Some("txtAirPlay")))
            });
            if asked_txt {
                return plist(&dict([(
                    "txtAirPlay",
                    Value::Data(txt_bytes(&self.ctx.txt)),
                )]));
            }
            return plist(&self.info());
        }
        if req.method == "SETUP" {
            return self.setup(req).await;
        }
        if req.method == "RECORD" && !self.started {
            self.started = true;
            println!(
                "{TAG} {} ({}) started",
                self.sender.name.as_deref().unwrap_or("?"),
                self.peer
            );
            self.ctx.media.started(
                self.id,
                self.close.clone(),
                self.sender.clone(),
                self.remote.clone(),
            );
        }
        if req.method == "SET_PARAMETER"
            && let Some(meta) = metadata_of(req, self.rate)
        {
            self.tell(meta);
        }
        if req.method == "SET_PARAMETER"
            && let Some(db) = volume_of(&req.body)
        {
            self.ctx.media.volume(db);
        }
        if path.ends_with("/command") {
            for meta in command_meta(&req.body) {
                self.tell(meta);
            }
        }
        if req.method == "TEARDOWN"
            && self.audio
            && bplist::decode(&req.body).ok().is_some_and(|b| {
                b.get("streams").and_then(Value::as_array).is_some_and(|s| {
                    s.iter().any(|sd| {
                        sd.get("type").and_then(Value::as_int) == Some(STREAM_REALTIME_AUDIO)
                    })
                })
            })
        {
            self.audio = false;
            self.ctx.media.close_audio();
            self.tell(AirplayMeta::Playing(false));
        }
        if req.method == "GET_PARAMETER" && req.body.starts_with(b"volume") {
            return Response {
                headers: vec![("Content-Type".into(), "text/parameters".into())],
                body: format!("volume: {:.6}\r\n", self.ctx.media.volume_db()).into_bytes(),
                ..Default::default()
            };
        }
        status(200)
    }

    async fn pair_setup(&mut self, req: &Request) -> Response {
        let flags = tlv8::decode(&req.body).get(&TLV_FLAGS).cloned();
        if let Some(flags) = flags {
            self.transient = flags.first().is_some_and(|f| f & FLAG_TRANSIENT != 0);
        }
        if let Some(session) = self.mfi4.as_mut() {
            let (pair_setup, ctx) = (&mut self.pair_setup, &self.ctx);
            let mut through =
                |data: &[u8]| Ok(pair_setup.handle(data, &ctx.identity, &ctx.pairings));
            let result = session.handle(&req.body, &ctx.helper, &mut through).await;
            return octet(result);
        }
        pairing(
            self.pair_setup
                .handle(&req.body, &self.ctx.identity, &self.ctx.pairings),
        )
    }

    async fn auth_setup(&mut self, req: &Request) -> Response {
        if header_number(req, "x-apple-at") == Some(mfi4::AUTH_TYPE) {
            let session = self
                .mfi4
                .get_or_insert_with(|| mfi4::Session::new(self.ctx.pairings.dir()));
            if header_number(req, "x-apple-pairingtranscript").is_some_and(|v| v != 0) {
                session.binds_pairing = true;
            }
            let pair_verify = &self.pair_verify;
            let mut check = |shown: &[u8]| {
                let (expected, ours) = pair_verify
                    .transcript_proofs()
                    .ok_or(TunnelError::NotVerified)?;
                if shown.get(..expected.len()) != Some(&expected[..]) {
                    return Err(TunnelError::ProofMismatch);
                }
                Ok(ours.to_vec())
            };
            let result = session
                .handle(&req.body, &self.ctx.helper, &mut check)
                .await;
            return octet(result);
        }
        match auth_setup::handle(&req.body, &self.ctx.helper)
            .await
            .transpose()
        {
            Some(result) => octet(result),
            None => status(400),
        }
    }

    fn info(&self) -> Value {
        let ctx = &self.ctx;
        let display = dict([
            ("uuid", Value::String(DISPLAY_UUID.into())),
            ("widthPhysical", Value::Int(0)),
            ("heightPhysical", Value::Int(0)),
            ("widthPixels", Value::Int(1920)),
            ("heightPixels", Value::Int(1080)),
            ("width", Value::Int(1920)),
            ("height", Value::Int(1080)),
            ("maxFPS", Value::Int(60)),
            ("refreshRate", Value::Real(1.0 / 60.0)),
            ("overscanned", Value::Bool(false)),
            ("features", Value::Int(14)),
            ("rotation", Value::Bool(false)),
        ]);
        dict([
            ("deviceID", Value::String(ctx.device_id.clone())),
            ("macAddress", Value::String(ctx.device_id.clone())),
            ("features", Value::Int(ctx.features)),
            ("statusFlags", Value::Int(68)),
            ("model", Value::String("LIVI".into())),
            ("manufacturer", Value::String("LIVI".into())),
            ("name", Value::String(ctx.name.clone())),
            ("sourceVersion", Value::String(ctx.source_version.clone())),
            ("protocolVersion", Value::String("1.1".into())),
            ("pk", Value::Data(ctx.identity.public.to_vec())),
            ("pi", Value::String(ctx.identity.pairing_id.clone())),
            ("vv", Value::Int(2)),
            ("keepAliveLowPower", Value::Bool(true)),
            ("keepAliveSendStatsAsBody", Value::Bool(true)),
            ("displays", Value::Array(vec![display])),
            (
                "supportedFormats",
                dict([
                    ("audioStream", Value::Int(audio_formats(ctx.sample_rate))),
                    ("screenStream", Value::Int(audio_formats(ctx.sample_rate))),
                ]),
            ),
        ])
    }

    fn event_keys(&self) -> Option<([u8; 32], [u8; 32])> {
        if self.transient {
            return self
                .pair_setup
                .session_key()
                .map(|k| session_keys::events(&k));
        }
        self.pair_verify
            .shared_secret()
            .map(|k| session_keys::events(&k))
    }

    fn stream_key(&self, sd: &Value) -> Option<[u8; 32]> {
        let k = self.pair_setup.session_key()?;
        let id = crate::stack::id_text(sd.get("streamConnectionID"));
        Some(session_keys::data_stream(&k, &id, session_keys::DATA_OUT))
    }

    async fn setup(&mut self, req: &Request) -> Response {
        let Ok(body) = bplist::decode(&req.body) else {
            return status(400);
        };
        let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_string);
        if let Some(name) = text("name") {
            self.sender.name = Some(name);
        }
        if let Some(model) = text("model") {
            self.sender.model = Some(model);
        }
        if let Some(streams) = body.get("streams").and_then(Value::as_array) {
            let mut out = Vec::new();
            for sd in streams {
                let kind = sd.get("type").and_then(Value::as_int).unwrap_or(0);
                if kind != STREAM_REALTIME_AUDIO && kind != STREAM_SCREEN {
                    println!("{TAG} stream type {kind} is not supported");
                    continue;
                }
                let shk = match sd.get("shk") {
                    Some(Value::Data(d)) => <[u8; 32]>::try_from(d.as_slice()).ok(),
                    _ => None,
                };
                let Some(key) = shk.or_else(|| self.stream_key(sd)) else {
                    println!("{TAG} stream type {kind} has no key");
                    continue;
                };
                if kind == STREAM_SCREEN {
                    let port = screen_proxy(key, self.ctx.media.clone());
                    out.push(dict([
                        ("type", Value::Int(kind)),
                        ("dataPort", Value::Int(u64::from(port))),
                    ]));
                    continue;
                }
                let format = AirplayFormat::of(sd);
                self.rate = format.rate;
                println!(
                    "{TAG} audio {:?} at {} Hz/{} bit (format {:?}, ct {:?}, spf {:?})",
                    format.codec,
                    format.rate,
                    format.bits,
                    sd.get("audioFormat").and_then(Value::as_int),
                    sd.get("ct").and_then(Value::as_int),
                    sd.get("spf").and_then(Value::as_int)
                );
                self.audio = true;
                self.tell(AirplayMeta::Playing(true));
                let (data, control) = self.ctx.media.open_audio(key, format).await;
                out.push(dict([
                    ("type", Value::Int(kind)),
                    ("dataPort", Value::Int(u64::from(data))),
                    ("controlPort", Value::Int(u64::from(control))),
                ]));
            }
            return plist(&dict([("streams", Value::Array(out))]));
        }
        if body.get("timingProtocol").and_then(Value::as_str) == Some("PTP") {
            let own = net::host_of(&self.local);
            return plist(&dict([
                (
                    "eventPort",
                    Value::Int(u64::from(event_port(
                        self.event_keys(),
                        self.commands.take(),
                    ))),
                ),
                ("timingPort", Value::Int(0)),
                (
                    "timingPeerInfo",
                    dict([
                        ("Addresses", Value::Array(vec![Value::String(own.clone())])),
                        ("ID", Value::String(own)),
                    ]),
                ),
            ]));
        }
        let timing_port = match TimingSync::listen() {
            Ok(mut timing) => {
                let port = timing.port();
                if let Some(peer_port) = body
                    .get("timingPort")
                    .and_then(Value::as_int)
                    .and_then(|p| u16::try_from(p).ok())
                    .filter(|&p| p > 0)
                {
                    let mut peer = self.peer;
                    peer.set_port(peer_port);
                    timing.start(peer);
                }
                self.timing = Some(timing);
                port
            }
            Err(e) => {
                println!("{TAG} timing: {e}");
                0
            }
        };
        plist(&dict([
            ("timingPort", Value::Int(u64::from(timing_port))),
            (
                "eventPort",
                Value::Int(u64::from(event_port(
                    self.event_keys(),
                    self.commands.take(),
                ))),
            ),
        ]))
    }
}

fn event_port(
    keys: Option<([u8; 32], [u8; 32])>,
    commands: Option<mpsc::UnboundedReceiver<AirplayCommand>>,
) -> u16 {
    let listener = match net::tcp_listener() {
        Ok(l) => l,
        Err(e) => {
            println!("{TAG} event port: {e}");
            return 0;
        }
    };
    let port = net::local_port(listener.local_addr());
    tokio::spawn(async move {
        let Ok(Ok((stream, _))) = tokio::time::timeout(CONNECT_WITHIN, listener.accept()).await
        else {
            return;
        };
        let (Some((read, write)), Some(commands)) = (keys, commands) else {
            drain(stream).await;
            return;
        };
        events(stream, ControlCipher::new(read, write), commands).await;
    });
    port
}

async fn drain(mut stream: TcpStream) {
    let mut buf = vec![0u8; 4096];
    while matches!(stream.read(&mut buf).await, Ok(n) if n > 0) {}
}

async fn events(
    stream: TcpStream,
    mut cipher: ControlCipher,
    mut commands: mpsc::UnboundedReceiver<AirplayCommand>,
) {
    let (mut rd, mut wr) = stream.into_split();
    let mut buf = vec![0u8; 4096];
    let (mut sealed, mut plain) = (Vec::new(), Vec::new());
    let mut cseq = 0u32;
    loop {
        tokio::select! {
            read = rd.read(&mut buf) => {
                let n = match read {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                sealed.extend_from_slice(&buf[..n]);
                let Ok((data, rest)) = cipher.decrypt(&sealed) else {
                    println!("{TAG} event channel decrypt failed");
                    return;
                };
                sealed = rest;
                plain.extend(data);
                let Ok((messages, rest)) = rtsp::parse(&plain) else {
                    return;
                };
                plain = rest;
                for msg in messages {
                    if msg.method.starts_with("RTSP/") {
                        if msg.path != "200" {
                            println!("{TAG} remote answered {}", msg.path);
                        }
                        continue;
                    }
                    let answer = cipher.encrypt(&rtsp::build_response(&msg, status(200)));
                    if wr.write_all(&answer).await.is_err() {
                        return;
                    }
                }
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    return;
                };
                cseq += 1;
                match command {
                    AirplayCommand::Volume(_) => {}
                    AirplayCommand::Seek(s) => println!("{TAG} remote seek {s:.1} s"),
                    _ => println!("{TAG} remote {}", command.code()),
                }
                let sent = cipher.encrypt(&remote_command(command, cseq));
                if wr.write_all(&sent).await.is_err() {
                    return;
                }
            }
        }
    }
}

const MR_SEEK: &str = "24";

fn remote_body(command: AirplayCommand) -> Value {
    if let AirplayCommand::Seek(seconds) = command {
        return dict([
            ("type", Value::String("sendMediaRemoteCommand".into())),
            ("modernMediaRemoteCommand", Value::String(MR_SEEK.into())),
            (
                "params",
                dict([(
                    "kMRMediaRemoteOptionPlaybackPosition",
                    Value::Real(seconds.max(0.0)),
                )]),
            ),
        ]);
    }
    let mut body = vec![
        (
            "type".to_string(),
            Value::String("sendMediaRemoteCommand".into()),
        ),
        ("value".to_string(), Value::String(command.code().into())),
    ];
    if let AirplayCommand::Volume(level) = command {
        let level = level.clamp(0.0, 1.0);
        body.push(("volume".into(), Value::Real(level)));
        body.push(("params".into(), dict([("volume", Value::Real(level))])));
    }
    Value::Dict(body)
}

fn remote_command(command: AirplayCommand, cseq: u32) -> Vec<u8> {
    let body = bplist::encode(&remote_body(command));
    let mut msg = format!(
        "POST /command RTSP/1.0\r\nContent-Type: {PLIST}\r\nContent-Length: {}\r\nCSeq: {cseq}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    msg.extend_from_slice(&body);
    msg
}

fn screen_proxy(key: [u8; 32], media: Arc<dyn AirplayMedia>) -> u16 {
    let listener = match net::tcp_listener() {
        Ok(l) => l,
        Err(e) => {
            println!("{TAG} screen: {e}");
            return 0;
        }
    };
    let port = net::local_port(listener.local_addr());
    tokio::spawn(async move {
        let Ok(Ok((mut phone, _))) = tokio::time::timeout(CONNECT_WITHIN, listener.accept()).await
        else {
            println!("{TAG} screen: the phone did not connect");
            return;
        };
        let port = media.open_screen(key).await;
        if port == 0 {
            println!("{TAG} screen: no receiver");
            return;
        }
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(mut receiver) => {
                let _ = receiver.set_nodelay(true);
                let _ = tokio::io::copy(&mut phone, &mut receiver).await;
            }
            Err(e) => println!("{TAG} screen: receiver :{port}: {e}"),
        }
        media.close_screen();
    });
    port
}

fn txt_bytes(txt: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in txt {
        let bytes = &entry.as_bytes()[..entry.len().min(255)];
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
    out
}

fn metadata_of(req: &Request, rate: u32) -> Option<AirplayMeta> {
    let kind = req.headers.get("content-type")?.trim();
    match kind {
        "application/x-dmap-tagged" => Some(dmap_track(&req.body)),
        "image/none" => Some(AirplayMeta::Artwork(Vec::new())),
        k if k.starts_with("image/") => Some(AirplayMeta::Artwork(req.body.clone())),
        "text/parameters" => progress(&req.body, rate),
        _ => None,
    }
}

fn dmap_items(mut b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    while b.len() >= 8 {
        let tag = [b[0], b[1], b[2], b[3]];
        let len = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
        let Some(value) = b.get(8..8 + len) else {
            break;
        };
        out.push((tag, value));
        b = &b[8 + len..];
    }
    out
}

fn dmap_track(body: &[u8]) -> AirplayMeta {
    let top = dmap_items(body);
    let items = match top.iter().find(|(t, _)| t == b"mlit") {
        Some((_, inner)) => dmap_items(inner),
        None => top,
    };
    let text = |tag: &[u8; 4]| {
        items
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    };
    AirplayMeta::Track {
        title: text(b"minm"),
        artist: text(b"asar"),
        album: text(b"asal"),
    }
}

fn progress(body: &[u8], rate: u32) -> Option<AirplayMeta> {
    let text = std::str::from_utf8(body).ok()?;
    let rest = text.trim().strip_prefix("progress:")?;
    let mut parts = rest.trim().split('/').map(|p| p.trim().parse::<u64>().ok());
    let (start, now, end) = (parts.next()??, parts.next()??, parts.next()??);
    let per_ms = f64::from(rate) / 1000.0;
    let span = |a: u64, b: u64| f64::from((b as u32).wrapping_sub(a as u32)) / per_ms;
    Some(AirplayMeta::Progress {
        elapsed_ms: span(start, now),
        duration_ms: span(start, end),
    })
}

const MR_PLAYING: u64 = 1;
const NOW_PLAYING_KEY: &str = "kMRMediaRemoteNowPlayingInfo";

fn command_meta(body: &[u8]) -> Vec<AirplayMeta> {
    let Ok(command) = bplist::decode(body) else {
        return Vec::new();
    };
    let params = command.get("params");
    match command.get("type").and_then(Value::as_str) {
        Some("updateMRPlaybackState") => params
            .and_then(|p| p.get("mrPlaybackState"))
            .and_then(Value::as_int)
            .map(|state| vec![AirplayMeta::Playing(state == MR_PLAYING)])
            .unwrap_or_default(),
        Some("updateMRNowPlayingInfo") => params
            .and_then(|p| p.get("params"))
            .map(now_playing_info)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn now_playing_info(info: &Value) -> Vec<AirplayMeta> {
    let key = |name: &str| info.get(&format!("{NOW_PLAYING_KEY}{name}"));
    let text = |name: &str| key(name).and_then(Value::as_str).map(str::to_string);
    let seconds = |name: &str| match key(name)? {
        Value::Real(v) => Some(*v),
        Value::Int(v) => Some(*v as f64),
        _ => None,
    };
    let mut out = Vec::new();
    let (title, artist, album) = (text("Title"), text("Artist"), text("Album"));
    if title.is_some() || artist.is_some() || album.is_some() {
        out.push(AirplayMeta::Track {
            title,
            artist,
            album,
        });
    }
    if let Some(art) = key("ArtworkData").and_then(Value::as_data) {
        out.push(AirplayMeta::Artwork(art.to_vec()));
    }
    if let (Some(elapsed), Some(duration)) = (seconds("ElapsedTime"), seconds("Duration")) {
        out.push(AirplayMeta::Progress {
            elapsed_ms: elapsed * 1000.0,
            duration_ms: duration * 1000.0,
        });
    }
    if let Some(rate) = seconds("PlaybackRate") {
        out.push(AirplayMeta::Playing(rate > 0.0));
    }
    out
}

fn volume_of(body: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(body).ok()?;
    text.trim().strip_prefix("volume:")?.trim().parse().ok()
}

fn header_number(req: &Request, name: &str) -> Option<u32> {
    let v = req.headers.get(name)?.trim();
    match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => v.parse().ok(),
    }
}

fn status(code: u16) -> Response {
    Response {
        status: Some(code),
        ..Default::default()
    }
}

fn pairing(body: Vec<u8>) -> Response {
    Response {
        headers: vec![("Content-Type".into(), "application/pairing+tlv8".into())],
        body,
        ..Default::default()
    }
}

fn octet(result: Result<Vec<u8>, impl std::fmt::Display>) -> Response {
    match result {
        Ok(body) => Response {
            headers: vec![("Content-Type".into(), "application/octet-stream".into())],
            body,
            ..Default::default()
        },
        Err(e) => {
            println!("{TAG} handler error: {e}");
            status(500)
        }
    }
}

fn plist(body: &Value) -> Response {
    Response {
        headers: vec![(
            "Content-Type".into(),
            "application/x-apple-binary-plist".into(),
        )],
        body: bplist::encode(body),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(tag: &[u8; 4], value: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    #[test]
    fn a_dmap_listing_gives_title_artist_and_album() {
        let inner = [
            item(b"mper", &[0; 8]),
            item(b"minm", b"Song"),
            item(b"asar", b"Band"),
            item(b"asal", b"Record"),
        ]
        .concat();
        assert_eq!(
            dmap_track(&item(b"mlit", &inner)),
            AirplayMeta::Track {
                title: Some("Song".into()),
                artist: Some("Band".into()),
                album: Some("Record".into()),
            }
        );
    }

    #[test]
    fn progress_counts_rtp_ticks_from_the_start() {
        assert_eq!(
            progress(b"progress: 1000/45100/442000\r\n", 44_100),
            Some(AirplayMeta::Progress {
                elapsed_ms: 1000.0,
                duration_ms: 10000.0,
            })
        );
    }

    #[test]
    fn progress_survives_a_wrapped_timestamp() {
        let start = u64::from(u32::MAX) - 43;
        let text = format!("progress: {start}/44056/88156");
        assert_eq!(
            progress(text.as_bytes(), 44_100),
            Some(AirplayMeta::Progress {
                elapsed_ms: 1000.0,
                duration_ms: 2000.0,
            })
        );
    }

    #[test]
    fn progress_follows_the_stream_rate() {
        assert_eq!(
            progress(b"progress: 0/48000/96000\r\n", 48_000),
            Some(AirplayMeta::Progress {
                elapsed_ms: 1000.0,
                duration_ms: 2000.0,
            })
        );
    }

    #[test]
    fn a_volume_parameter_is_no_metadata() {
        assert_eq!(progress(b"volume: -20.000000\r\n", 44_100), None);
    }

    fn command(kind: &str, params: Value) -> Vec<u8> {
        bplist::encode(&dict([
            ("type", Value::String(kind.into())),
            ("params", params),
        ]))
    }

    #[test]
    fn media_remote_now_playing_gives_track_artwork_progress_and_rate() {
        let info = dict([
            (
                "kMRMediaRemoteNowPlayingInfoTitle",
                Value::String("Holes".into()),
            ),
            (
                "kMRMediaRemoteNowPlayingInfoArtist",
                Value::String("Mercury Rev".into()),
            ),
            (
                "kMRMediaRemoteNowPlayingInfoArtworkData",
                Value::Data(vec![0xff, 0xd8]),
            ),
            ("kMRMediaRemoteNowPlayingInfoElapsedTime", Value::Real(2.5)),
            ("kMRMediaRemoteNowPlayingInfoDuration", Value::Real(351.0)),
            ("kMRMediaRemoteNowPlayingInfoPlaybackRate", Value::Real(1.0)),
        ]);
        let body = command(
            "updateMRNowPlayingInfo",
            dict([("type", Value::String("npi-text".into())), ("params", info)]),
        );
        assert_eq!(
            command_meta(&body),
            vec![
                AirplayMeta::Track {
                    title: Some("Holes".into()),
                    artist: Some("Mercury Rev".into()),
                    album: None,
                },
                AirplayMeta::Artwork(vec![0xff, 0xd8]),
                AirplayMeta::Progress {
                    elapsed_ms: 2500.0,
                    duration_ms: 351_000.0,
                },
                AirplayMeta::Playing(true),
            ]
        );
    }

    #[test]
    fn media_remote_playback_state_one_is_playing() {
        let state = |s| {
            command_meta(&command(
                "updateMRPlaybackState",
                dict([("mrPlaybackState", Value::Int(s))]),
            ))
        };
        assert_eq!(state(1), vec![AirplayMeta::Playing(true)]);
        assert_eq!(state(2), vec![AirplayMeta::Playing(false)]);
    }

    #[test]
    fn an_empty_now_playing_info_gives_nothing() {
        let body = command("updateMRNowPlayingInfo", dict([("params", dict([]))]));
        assert!(command_meta(&body).is_empty());
    }

    #[test]
    fn a_volume_parameter_gives_decibels() {
        assert_eq!(volume_of(b"volume: -15.000000\r\n"), Some(-15.0));
        assert_eq!(volume_of(b"volume: -144.000000\r\n"), Some(-144.0));
        assert_eq!(volume_of(b"progress: 1/2/3\r\n"), None);
    }

    #[test]
    fn remote_command_is_a_media_remote_post() {
        let (msgs, rest) = rtsp::parse(&remote_command(AirplayCommand::Next, 3)).unwrap();
        assert!(rest.is_empty());
        let msg = &msgs[0];
        assert_eq!(
            (msg.method.as_str(), msg.path.as_str()),
            ("POST", "/command")
        );
        assert_eq!(msg.headers.get("cseq").map(String::as_str), Some("3"));
        let body = bplist::decode(&msg.body).unwrap();
        assert_eq!(
            body.get("type").and_then(Value::as_str),
            Some("sendMediaRemoteCommand")
        );
        assert_eq!(body.get("value").and_then(Value::as_str), Some("nitm"));
    }

    #[test]
    fn volume_goes_as_unit_level() {
        let body = remote_body(AirplayCommand::Volume(0.4));
        assert_eq!(body.get("value").and_then(Value::as_str), Some("dvlc"));
        assert!(matches!(body.get("volume"), Some(Value::Real(v)) if *v == 0.4));
        assert!(matches!(
            body.get("params").and_then(|p| p.get("volume")),
            Some(Value::Real(v)) if *v == 0.4
        ));
        assert!(matches!(
            remote_body(AirplayCommand::Volume(1.5)).get("volume"),
            Some(Value::Real(v)) if *v == 1.0
        ));
    }

    #[test]
    fn the_clock_runs_only_while_playing_and_stops_at_the_end() {
        let at = Instant::now();
        let mut clock = Clock {
            elapsed_ms: 1000.0,
            duration_ms: 3000.0,
            at,
            running: true,
        };
        assert_eq!(clock.elapsed_at(at + Duration::from_secs(1)), 2000.0);
        assert_eq!(clock.elapsed_at(at + Duration::from_secs(5)), 3000.0);
        clock.running = false;
        assert_eq!(clock.elapsed_at(at + Duration::from_secs(1)), 1000.0);
    }

    #[test]
    fn seek_is_media_remote_command_24_in_seconds() {
        let body = remote_body(AirplayCommand::Seek(61.5));
        assert_eq!(
            body.get("modernMediaRemoteCommand").and_then(Value::as_str),
            Some("24")
        );
        assert!(matches!(
            body.get("params")
                .and_then(|p| p.get("kMRMediaRemoteOptionPlaybackPosition")),
            Some(Value::Real(v)) if *v == 61.5
        ));
    }

    #[test]
    fn the_audio_format_comes_from_the_senders_choice() {
        let sd = |entries: Vec<(&str, u64)>| {
            Value::Dict(
                entries
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Value::Int(v)))
                    .collect(),
            )
        };
        let of = |entries| {
            let f = AirplayFormat::of(&sd(entries));
            (f.codec, f.rate, f.bits)
        };
        assert_eq!(
            of(vec![("audioFormat", ALAC_44K_24), ("ct", 2)]),
            (AirplayCodec::Alac, 44_100, 24)
        );
        assert_eq!(
            of(vec![("audioFormat", ALAC_48K), ("ct", 2)]),
            (AirplayCodec::Alac, 48_000, 16)
        );
        assert_eq!(
            of(vec![("audioFormat", AAC_ELD_44K), ("ct", 8)]),
            (AirplayCodec::AacEld, 44_100, 16)
        );
        assert_eq!(of(vec![("ct", 2)]), (AirplayCodec::Alac, 44_100, 16));
        assert_eq!(audio_formats(44_100), (1 << 18) | (1 << 19) | (1 << 24));
    }
}
