// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::crypto::{X25519, chacha_seal, ed25519_public, ed25519_sign, nonce_label, random_bytes};
use crate::identity::load_or_create;
use crate::media::AudioStream;
use crate::opack;
use crate::pair_setup::tlv;
use crate::pairings::tests::TempDir;
use crate::tlv8;

pub(crate) struct FakeMedia {
    calls: Mutex<Vec<String>>,
    pub(crate) started: broadcast::Sender<(u32, u32)>,
    pub(crate) video_status: Mutex<Option<crate::media::VideoStatus>>,
    pub(crate) output_delay: Mutex<Option<u32>>,
}

impl FakeMedia {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::default(),
            started: broadcast::channel(8).0,
            video_status: Mutex::default(),
            output_delay: Mutex::default(),
        })
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
        async {
            AudioStream {
                id: 9,
                data_port: 6001,
                control_port: 6002,
            }
        }
    }
    fn set_audio_active(&self, stream: u32, active: bool) {
        self.log(format!("audio_active {stream} {active}"));
    }
    fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32) {
        self.log(format!("volume {stream} {level} {ramp_ms}"));
    }
    fn audio_output_delay(&self, _stream: u32) -> Option<u32> {
        *self.output_delay.lock().unwrap()
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
    fn play_video(&self, url: String, audio_device: String, muted: bool) {
        let muted = if muted { " muted" } else { "" };
        self.log(format!("play_video {url} {audio_device}{muted}"));
    }
    fn set_video_rate(&self, rate: f64) {
        self.log(format!("video_rate {rate}"));
    }
    fn seek_video(&self, seconds: f64) {
        self.log(format!("seek_video {seconds}"));
    }
    fn set_video_muted(&self, muted: bool) {
        self.log(format!("video_muted {muted}"));
    }
    fn stop_video(&self) {
        self.log("stop_video".into());
    }
    fn set_video_shown(&self, shown: bool) {
        self.log(format!("show_video {shown}"));
    }
    fn video_status(&self) -> Option<crate::media::VideoStatus> {
        *self.video_status.lock().unwrap()
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
        Self {
            sock,
            cipher: None,
            buf: Vec::new(),
            cseq: 0,
            shared: [0; 32],
        }
    }

    pub(crate) async fn request(
        &mut self,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> (String, Vec<u8>) {
        self.request_with(method, path, "", body).await
    }

    pub(crate) async fn request_with(
        &mut self,
        method: &str,
        path: &str,
        headers: &str,
        body: &[u8],
    ) -> (String, Vec<u8>) {
        self.cseq += 1;
        let mut msg = format!(
            "{method} {path} RTSP/1.0\r\nCSeq: {}\r\n{headers}Content-Length: {}\r\n\r\n",
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
        if body.is_empty() {
            Value::Dict(Vec::new())
        } else {
            bplist::decode(&body).unwrap()
        }
    }

    pub(crate) async fn verify(&mut self, secret: &[u8; 32], id: &[u8]) {
        let eph = X25519::generate();
        let eph_public = eph.public;
        let m1 = tlv8::encode(&[(tlv::STATE, &[1]), (tlv::PUBLIC_KEY, &eph_public)]);
        let (_, m2) = self.request("POST", "/pair-verify", &m1).await;
        let m2 = tlv8::decode(&m2);
        let acc: [u8; 32] = m2[&tlv::PUBLIC_KEY].as_slice().try_into().unwrap();
        let shared = eph.shared(&acc).unwrap();
        let enc = hkdf_sha512(
            &shared,
            b"Pair-Verify-Encrypt-Salt",
            b"Pair-Verify-Encrypt-Info",
        );
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
        Self::with_video(false, false).await
    }

    async fn with_video(video: bool, capture: bool) -> Self {
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
            hls_video: video,
            vehicle_climate: true,
            app_version: "9.5.0".into(),
            app_build_date: String::new(),
            cluster_url: String::new(),
            vehicle_variant: String::new(),
            engines: Default::default(),
            cluster_content: None,
            images: Vec::new(),
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
            video_capture: capture.then(|| dir.0.join("video")),
        });
        let listener = net::tcp_listener().unwrap();
        let port = net::local_port(listener.local_addr());
        let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let (cmds, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, events) = mpsc::unbounded_channel();
        tokio::spawn(run(accepted, ctx, cmd_rx, ev_tx));
        let phone = Phone {
            sock,
            cipher: None,
            buf: Vec::new(),
            cseq: 0,
            shared: [0; 32],
        };
        Self {
            _dir: dir,
            media,
            phone,
            cmds,
            events,
            helper_lines,
            secret,
        }
    }

    async fn event(&mut self) -> StackEvent {
        tokio::time::timeout(Duration::from_secs(5), self.events.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

fn int_of(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_int).unwrap()
}

#[test]
fn the_feedback_names_the_sample_the_speaker_plays_now() {
    assert_eq!(
        sounding_sample(100, 0.5, 1000, 48_000),
        100,
        "still in the jitter buffer"
    );
    assert_eq!(sounding_sample(100, 2.0, 1000, 48_000), 100 + 48_000);
    assert_eq!(
        sounding_sample(100, 2.0, 1300, 48_000),
        100 + 33_600,
        "the measured output delay moves it back"
    );
    assert_eq!(
        sounding_sample(u32::MAX, 2.0, 1000, 48_000),
        47_999,
        "RTP time wraps"
    );
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
    let info = rig
        .phone
        .plist("GET", "/info", &dict([("qualifier", Value::Bool(true))]))
        .await;
    assert!(info.get("displays").is_some());
    let (code, _) = rig.phone.request("OPTIONS", "*", b"").await;
    assert_eq!(code, "200");
    let (code, _) = rig.phone.request("GET", "/whatever", b"").await;
    assert_eq!(code, "200");
    let (code, _) = rig.phone.request("SETUP", "/s", b"nope").await;
    assert_eq!(code, "400");
    let screen = dict([(
        "streams",
        Value::Array(vec![dict([("type", Value::Int(110))])]),
    )]);
    let (code, _) = rig
        .phone
        .request("SETUP", "/s", &bplist::encode(&screen))
        .await;
    assert_eq!(code, "500");
    let (code, _) = rig.phone.request("POST", "/auth-setup", b"short").await;
    assert_eq!(code, "400");
    let (code, _) = rig.phone.request("TEARDOWN", "/s", b"").await;
    assert_eq!(code, "200");
    let (code, _) = rig.phone.request("POST", "/feedback", b"").await;
    assert_eq!(code, "200");
    let (code, setup) = rig
        .phone
        .request("POST", "/pair-setup", &tlv8::encode(&[(tlv::STATE, &[1])]))
        .await;
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
    let resp = rig
        .phone
        .plist("SETUP", "/s", &dict([("timingPort", Value::Int(9))]))
        .await;

    tokio::time::sleep(PHONE_SILENT * 2).await;
    let (code, _) = rig.phone.request("POST", "/feedback", b"").await;
    assert_eq!(code, "200");

    let clock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let timing_port = int_of(&resp, "timingPort") as u16;
    clock
        .send_to(
            &[0u8; crate::timing::PACKET_LEN],
            ("127.0.0.1", timing_port),
        )
        .await
        .unwrap();
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
async fn a_queued_video_plays_on_the_host_and_follows_the_phone() {
    let mut rig = Rig::with_video(true, false).await;
    let secret = rig.secret;
    rig.phone.verify(&secret, b"phone-1").await;
    let offered = Value::Array(vec![Value::String(video_playback::FEATURE.into())]);
    let session = dict([("timingPort", Value::Int(9)), ("features", offered)]);
    rig.phone.plist("SETUP", "/s", &session).await;
    let video = dict([
        ("type", Value::Int(STREAM_DATA)),
        (
            "clientTypeUUID",
            Value::String(video_playback::DATASTREAM_UUID.to_lowercase()),
        ),
        ("clientUUID", Value::String("client-1".into())),
    ]);
    rig.phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![video]))]),
        )
        .await;
    let offer = dict([
        ("type", Value::String("setProxyParameters".into())),
        (
            "params",
            dict([
                ("proxyUrl", Value::String("https://[fe80::1]".into())),
                ("proxyPort", Value::Int(60664)),
                ("proxyPsk", Value::Data(vec![1; 32])),
                ("proxyPskIdentity", Value::Data(vec![2; 16])),
                ("proxyAuthorization", Value::String("Basic eDp5".into())),
            ]),
        ),
    ]);
    rig.phone
        .request("POST", "/command", &bplist::encode(&offer))
        .await;

    let kind = |t: &str| ("type", Value::String(t.into()));
    let item = dict([(
        "Content-Location",
        Value::String("http://127.0.0.1:58539/mirror/index.m3u8".into()),
    )]);
    let time = dict([("value", Value::Int(1200)), ("timescale", Value::Int(100))]);
    let muted = ("property", Value::String("muted".into()));
    for message in [
        dict([kind("setRate"), ("rate", Value::Real(0.0))]),
        dict([kind("insertPlayQueueItem"), ("item", item)]),
        dict([kind("setRate"), ("rate", Value::Real(0.0))]),
        dict([kind("seek"), ("time", time)]),
        dict([
            kind("setProperty"),
            muted.clone(),
            ("value", Value::Bool(true)),
        ]),
        dict([kind("setProperty"), muted, ("value", Value::Int(1))]),
        dict([kind("stop")]),
    ] {
        let body = dict([(
            "params",
            dict([("data", Value::Data(bplist::encode(&message)))]),
        )]);
        let (code, _) = rig
            .phone
            .request_with(
                "POST",
                "/command",
                "X-Apple-StreamID: 2\r\n",
                &bplist::encode(&body),
            )
            .await;
        assert_eq!(code, "200");
    }

    let calls: Vec<String> = rig
        .media
        .calls()
        .into_iter()
        .filter(|c| c.contains("video"))
        .map(|c| {
            let tunneled = c.starts_with("play_video http://127.0.0.1:")
                && c.ends_with("/mirror/index.m3u8 out")
                && !c.contains(":58539/");
            if tunneled { "play".into() } else { c }
        })
        .collect();
    assert_eq!(
        calls,
        [
            "play",
            "video_rate 0",
            "seek_video 12",
            "video_muted true",
            "stop_video"
        ]
    );
    assert!(
        !rig._dir.0.join("video").exists(),
        "without debug nothing is kept"
    );
}

#[tokio::test]
async fn a_muted_item_starts_muted() {
    let mut rig = Rig::with_video(true, false).await;
    let secret = rig.secret;
    rig.phone.verify(&secret, b"phone-1").await;
    let offered = Value::Array(vec![Value::String(video_playback::FEATURE.into())]);
    let session = dict([("timingPort", Value::Int(9)), ("features", offered)]);
    rig.phone.plist("SETUP", "/s", &session).await;
    let video = dict([
        ("type", Value::Int(STREAM_DATA)),
        (
            "clientTypeUUID",
            Value::String(video_playback::DATASTREAM_UUID.to_lowercase()),
        ),
    ]);
    rig.phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![video]))]),
        )
        .await;
    let media = dict([
        ("type", Value::Int(102)),
        ("streamConnectionID", Value::Int(44)),
        ("audioType", Value::String("media".into())),
        ("audioFormat", Value::Int(0x10)),
        ("audioLatencyMs", Value::Int(1000)),
    ]);
    rig.phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![media]))]),
        )
        .await;
    let offer = dict([
        ("type", Value::String("setProxyParameters".into())),
        (
            "params",
            dict([
                ("proxyUrl", Value::String("https://[fe80::1]".into())),
                ("proxyPort", Value::Int(60664)),
                ("proxyPsk", Value::Data(vec![1; 32])),
                ("proxyPskIdentity", Value::Data(vec![2; 16])),
                ("proxyAuthorization", Value::String("Basic eDp5".into())),
            ]),
        ),
    ]);
    rig.phone
        .request("POST", "/command", &bplist::encode(&offer))
        .await;

    let item = dict([
        (
            "Content-Location",
            Value::String("http://127.0.0.1:58539/mirror/index.m3u8".into()),
        ),
        ("muted", Value::Bool(true)),
    ]);
    let message = dict([
        ("type", Value::String("insertPlayQueueItem".into())),
        ("item", item),
    ]);
    let body = bplist::encode(&dict([(
        "params",
        dict([("data", Value::Data(bplist::encode(&message)))]),
    )]));
    rig.phone
        .request_with("POST", "/command", "X-Apple-StreamID: 2\r\n", &body)
        .await;

    let plays: Vec<String> = rig
        .media
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("play_video"))
        .collect();
    assert_eq!(plays.len(), 1);
    assert!(plays[0].ends_with(" muted"), "{plays:?}");
}

#[tokio::test]
async fn leaving_park_hides_the_picture_and_the_sound_plays_on() {
    let mut rig = Rig::with_video(true, false).await;
    let secret = rig.secret;
    rig.phone.verify(&secret, b"phone-1").await;
    let offered = Value::Array(vec![Value::String(video_playback::FEATURE.into())]);
    let session = dict([("timingPort", Value::Int(9)), ("features", offered)]);
    rig.phone.plist("SETUP", "/s", &session).await;
    let video = dict([
        ("type", Value::Int(STREAM_DATA)),
        (
            "clientTypeUUID",
            Value::String(video_playback::DATASTREAM_UUID.to_lowercase()),
        ),
    ]);
    rig.phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![video]))]),
        )
        .await;
    let offer = dict([
        ("type", Value::String("setProxyParameters".into())),
        (
            "params",
            dict([
                ("proxyUrl", Value::String("https://[fe80::1]".into())),
                ("proxyPort", Value::Int(60664)),
                ("proxyPsk", Value::Data(vec![1; 32])),
                ("proxyPskIdentity", Value::Data(vec![2; 16])),
                ("proxyAuthorization", Value::String("Basic eDp5".into())),
            ]),
        ),
    ]);
    rig.phone
        .request("POST", "/command", &bplist::encode(&offer))
        .await;
    let queue = || {
        let item = dict([(
            "Content-Location",
            Value::String("http://127.0.0.1:58539/mirror/index.m3u8".into()),
        )]);
        let message = dict([
            ("type", Value::String("insertPlayQueueItem".into())),
            ("item", item),
        ]);
        bplist::encode(&dict([(
            "params",
            dict([("data", Value::Data(bplist::encode(&message)))]),
        )]))
    };
    let video_calls = |rig: &Rig| -> Vec<String> {
        let calls = rig
            .media
            .calls()
            .into_iter()
            .filter(|c| c.contains("_video"));
        calls
            .map(|c| c.split(' ').next().unwrap_or_default().to_string())
            .collect()
    };
    let allowed = |info: &Value| {
        let v = info
            .get("videoPlaybackInfo")
            .and_then(|v| v.get("videoPlaybackAllowed"));
        v.and_then(Value::as_bool)
    };
    let qualifier = dict([("qualifier", Value::Bool(true))]);

    rig.phone
        .request_with("POST", "/command", "X-Apple-StreamID: 2\r\n", &queue())
        .await;
    assert_eq!(video_calls(&rig), ["play_video"]);

    let wait_for = |n: usize| {
        let media = rig.media.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(5), async {
                while media
                    .calls()
                    .iter()
                    .filter(|c| c.contains("_video"))
                    .count()
                    < n
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
    };

    rig.cmds.send(StackCmd::VideoAllowed(false)).unwrap();
    wait_for(2).await;
    assert_eq!(video_calls(&rig), ["play_video", "show_video"]);
    assert!(rig.media.calls().contains(&"show_video false".to_string()));

    rig.phone
        .request_with("POST", "/command", "X-Apple-StreamID: 2\r\n", &queue())
        .await;
    assert_eq!(
        video_calls(&rig),
        ["play_video", "show_video", "stop_video", "play_video"]
    );
    assert_eq!(
        allowed(&rig.phone.plist("GET", "/info", &qualifier).await),
        Some(false)
    );
    let audio_only = dict([(
        "type",
        Value::String("changeVideoPlaybackToAudioOnly".into()),
    )]);
    let (code, _) = rig
        .phone
        .request("POST", "/command", &bplist::encode(&audio_only))
        .await;
    assert_eq!(code, "200");

    rig.cmds.send(StackCmd::VideoAllowed(false)).unwrap();
    rig.cmds.send(StackCmd::VideoAllowed(true)).unwrap();
    wait_for(5).await;
    assert_eq!(
        rig.media.calls().last().map(String::as_str),
        Some("show_video true")
    );
    assert_eq!(
        allowed(&rig.phone.plist("GET", "/info", &qualifier).await),
        Some(true)
    );
}

#[tokio::test]
async fn the_video_playback_trial_claims_the_feature_and_keeps_payloads() {
    let mut rig = Rig::with_video(true, true).await;
    let secret = rig.secret;
    rig.phone.verify(&secret, b"phone-1").await;
    let offered = ["hevc", video_playback::FEATURE].map(|f| Value::String(f.into()));
    let session = dict([
        ("timingPort", Value::Int(9)),
        ("features", Value::Array(offered.into())),
    ]);
    let resp = rig.phone.plist("SETUP", "/s", &session).await;
    let enabled = resp
        .get("enabledFeatures")
        .and_then(Value::as_array)
        .unwrap();
    assert!(enabled.contains(&Value::String(video_playback::FEATURE.into())));

    let info = rig
        .phone
        .plist("GET", "/info", &dict([("qualifier", Value::Bool(true))]))
        .await;
    let allowed = info
        .get("videoPlaybackInfo")
        .and_then(|v| v.get("videoPlaybackAllowed"));
    assert_eq!(allowed.and_then(Value::as_bool), Some(true));

    let video = dict([
        ("type", Value::Int(STREAM_DATA)),
        (
            "clientTypeUUID",
            Value::String(video_playback::DATASTREAM_UUID.to_lowercase()),
        ),
        ("clientUUID", Value::String("client-1".into())),
    ]);
    let out = rig
        .phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![video]))]),
        )
        .await;
    let stream = &out.get("streams").and_then(Value::as_array).unwrap()[0];
    assert_eq!(int_of(stream, "streamID"), 2);
    assert!(stream.get("dataPort").is_none());

    let payload = dict([("params", dict([("data", Value::Data(b"hello".to_vec()))]))]);
    let (code, _) = rig
        .phone
        .request_with(
            "POST",
            "/command",
            "X-Apple-StreamID: 2\r\n",
            &bplist::encode(&payload),
        )
        .await;
    assert_eq!(code, "200");
    let session_dir = std::fs::read_dir(rig._dir.0.join("video"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let kept = std::fs::read(session_dir.join("0001-stream2.bin")).unwrap();
    assert_eq!(kept, b"hello");

    let queue = |url: &str| {
        let item = dict([("Content-Location", Value::String(url.into()))]);
        let message = dict([
            ("type", Value::String("insertPlayQueueItem".into())),
            ("item", item),
        ]);
        bplist::encode(&dict([(
            "params",
            dict([("data", Value::Data(bplist::encode(&message)))]),
        )]))
    };
    let mine = "http://127.0.0.1:58539/mirror/index.m3u8";
    for body in [queue(mine), queue("http:///nothing")] {
        let (code, _) = rig
            .phone
            .request_with("POST", "/command", "X-Apple-StreamID: 2\r\n", &body)
            .await;
        assert_eq!(code, "200");
    }
    let offer = dict([
        ("type", Value::String("setProxyParameters".into())),
        (
            "params",
            dict([
                ("proxyUrl", Value::String("https://[fe80::1]".into())),
                ("proxyPort", Value::Int(60664)),
                ("proxyPsk", Value::Data(vec![1; 32])),
                ("proxyPskIdentity", Value::Data(vec![2; 16])),
                ("proxyAuthorization", Value::String("Basic eDp5".into())),
            ]),
        ),
    ]);
    let broken = dict([
        ("type", Value::String("setProxyParameters".into())),
        (
            "params",
            dict([("proxyUrl", Value::String("https://phone".into()))]),
        ),
    ]);
    for command in [broken, offer] {
        let (code, _) = rig
            .phone
            .request("POST", "/command", &bplist::encode(&command))
            .await;
        assert_eq!(code, "200");
    }
    for body in [queue(mine), queue("https://play.itunes.apple.com/a.m3u8")] {
        let (code, _) = rig
            .phone
            .request_with("POST", "/command", "X-Apple-StreamID: 2\r\n", &body)
            .await;
        assert_eq!(code, "200");
    }

    let settings = || {
        dict([(
            "streams",
            Value::Array(vec![dict([
                ("type", Value::Int(STREAM_DATA)),
                (
                    "clientTypeUUID",
                    Value::String(video_playback::SETTINGS_DATASTREAM_UUID.into()),
                ),
                ("seed", Value::Int(7)),
            ])]),
        )])
    };
    let out = rig.phone.plist("SETUP", "/s", &settings()).await;
    let stream = &out.get("streams").and_then(Value::as_array).unwrap()[0];
    assert_eq!(int_of(stream, "streamID"), 3);
    let port = int_of(stream, "dataPort") as u16;
    let again = rig.phone.plist("SETUP", "/s", &settings()).await;
    assert_eq!(
        again
            .get("streams")
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(0)
    );

    let key = hkdf_sha512(
        &rig.phone.shared,
        b"DataStream-Salt7",
        b"DataStream-Output-Encryption-Key",
    );
    let mut package = vec![0u8; 32];
    package[..4].copy_from_slice(&(32u32 + 4).to_be_bytes());
    package[16..20].copy_from_slice(b"comm");
    package.extend_from_slice(b"opak");
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    sock.write_all(&ControlCipher::new([0; 32], key).encrypt(&package))
        .await
        .unwrap();
    let kept = session_dir.join("0006-settings.bin");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !kept.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(kept).unwrap(), b"opak");
}

#[tokio::test]
async fn vehicle_state_is_claimed_described_and_answered_on_its_own_stream() {
    let mut rig = Rig::new().await;
    let secret = rig.secret;
    rig.phone.verify(&secret, b"phone-1").await;
    let warmer = serde_json::json!({ "driverTargetC": 23 })
        .as_object()
        .cloned()
        .unwrap();
    rig.cmds.send(StackCmd::Climate(warmer)).unwrap();
    let offered = Value::Array(vec![Value::String(vehicle_state::FEATURE.into())]);
    let session = dict([("timingPort", Value::Int(9)), ("features", offered)]);
    let resp = rig.phone.plist("SETUP", "/s", &session).await;
    let enabled = resp
        .get("enabledFeatures")
        .and_then(Value::as_array)
        .unwrap();
    assert!(enabled.contains(&Value::String(vehicle_state::FEATURE.into())));
    let climate = loop {
        if let StackEvent::Climate(c) = rig.event().await
            && c["driverTargetC"] == 23.0
        {
            break c;
        }
    };
    assert_eq!(
        climate["auto"], true,
        "LIVI's climate waits for the vehicle state"
    );
    let by_hand = serde_json::json!({ "fanLevel": 6 })
        .as_object()
        .cloned()
        .unwrap();
    rig.cmds.send(StackCmd::Climate(by_hand)).unwrap();
    let StackEvent::Climate(climate) = rig.event().await else {
        panic!("no climate")
    };
    assert_eq!(
        (&climate["fanLevel"], &climate["auto"]),
        (&6.into(), &false.into())
    );
    let info = rig
        .phone
        .plist("GET", "/info", &Value::Dict(Vec::new()))
        .await;
    let described = info.get("vehicleStateProtocolInfo").unwrap();
    assert_eq!(
        described.get("pluginCount").and_then(Value::as_int),
        Some(1)
    );
    assert!(info.get("displayPanels").is_some());

    let stream = dict([
        ("type", Value::Int(STREAM_DATA)),
        (
            "clientTypeUUID",
            Value::String(vehicle_state::HIGH_PRIORITY_DATASTREAM_UUID.into()),
        ),
        ("seed", Value::Int(11)),
    ]);
    let out = rig
        .phone
        .plist(
            "SETUP",
            "/s",
            &dict([("streams", Value::Array(vec![stream]))]),
        )
        .await;
    let stream = &out.get("streams").and_then(Value::as_array).unwrap()[0];
    assert_eq!(int_of(stream, "streamID"), 2);
    let port = int_of(stream, "dataPort") as u16;

    let key = |info: &[u8]| hkdf_sha512(&rig.phone.shared, b"DataStream-Salt11", info);
    let mut cipher = ControlCipher::new(
        key(b"DataStream-Input-Encryption-Key"),
        key(b"DataStream-Output-Encryption-Key"),
    );
    let ask = opack::dict([
        ("command", opack::Value::str("configRequest")),
        ("transactionID", opack::Value::Int(-5)),
    ]);
    let message = opack::encode(&opack::dict([
        ("pluginID", opack::Value::Int(0)),
        ("pluginData", opack::Value::Data(opack::encode(&ask))),
    ]));
    let mut package = vec![0u8; 32];
    package[..4].copy_from_slice(&(32 + message.len() as u32).to_be_bytes());
    package[4..8].copy_from_slice(b"asyn");
    package[16..20].copy_from_slice(b"cmnd");
    package.extend_from_slice(&message);
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    sock.write_all(&cipher.encrypt(&package)).await.unwrap();

    let (mut sealed, mut plain) = (Vec::new(), Vec::new());
    let mut chunk = vec![0u8; 16 * 1024];
    while plain.len() < 4
        || plain.len() < u32::from_be_bytes(plain[..4].try_into().unwrap()) as usize
    {
        let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut chunk))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0, "vehicle state stream closed");
        sealed.extend_from_slice(&chunk[..n]);
        let (data, rest) = cipher.decrypt(&sealed).unwrap();
        plain.extend(data);
        sealed = rest;
    }
    assert_eq!(&plain[4..8], b"asyn");
    assert_eq!(&plain[16..20], b"cmnd");
    let outer = opack::decode(&plain[32..]).unwrap();
    let answer = opack::decode(
        outer
            .get("pluginData")
            .and_then(opack::Value::as_data)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        answer.get("command").and_then(opack::Value::as_str),
        Some("configResponse")
    );
    assert_eq!(
        answer.get("transactionID").and_then(opack::Value::as_int),
        Some(-5)
    );
}

#[tokio::test]
async fn vehicle_state_is_always_described_and_enabled_only_when_offered() {
    for (offered, on) in [("hevc", false), (vehicle_state::FEATURE, true)] {
        let mut rig = Rig::new().await;
        let secret = rig.secret;
        rig.phone.verify(&secret, b"phone-1").await;
        let offered = Value::Array(vec![Value::String(offered.into())]);
        let session = dict([("timingPort", Value::Int(9)), ("features", offered)]);
        let resp = rig.phone.plist("SETUP", "/s", &session).await;
        let enabled = resp
            .get("enabledFeatures")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(
            enabled.contains(&Value::String(vehicle_state::FEATURE.into())),
            on
        );
        let info = rig
            .phone
            .plist("GET", "/info", &Value::Dict(Vec::new()))
            .await;
        assert!(info.get("vehicleStateProtocolInfo").is_some());
        assert!(info.get("displayPanels").is_some());
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
            dict([
                ("type", Value::Int(110)),
                ("streamConnectionID", Value::Int(42)),
            ]),
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
        StackEvent::VideoCodec {
            cluster: false,
            codec: ScreenCodec::H264
        }
    );
    assert_eq!(rig.event().await, StackEvent::MainScreenReady);

    let (code, _) = rig.phone.request("RECORD", "/s", b"").await;
    assert_eq!(code, "200");
    assert_eq!(
        rig.event().await,
        StackEvent::Active {
            ip: "127.0.0.1".into(),
            controller_id: Some("phone-1".into())
        }
    );

    rig.media.started.send((99, 1)).unwrap();
    rig.media.started.send((9, 100)).unwrap();
    let call = AudioProfile {
        kind: AudioKind::Call,
        label: "telephony".into(),
    };
    assert_eq!(
        rig.event().await,
        StackEvent::AudioActive {
            profile: call.clone(),
            active: true
        }
    );
    assert_eq!(
        rig.event().await,
        StackEvent::MicActive {
            active: true,
            rate: 16000,
            channels: 1
        }
    );

    let fb = rig.phone.plist("POST", "/feedback", &dict([])).await;
    let fb = &fb.get("streams").and_then(Value::as_array).unwrap()[0];
    assert_eq!(int_of(fb, "type"), 100);
    assert_eq!(int_of(fb, "sampleRate"), 16000);
    assert!(fb.get("sampleTime").is_some());

    let duck = dict([
        ("type", Value::String("duckAudio".into())),
        (
            "params",
            dict([("volume", Value::Int(0)), ("durationMs", Value::Int(500))]),
        ),
    ]);
    rig.phone.plist("POST", "/command", &duck).await;
    assert_eq!(
        rig.event().await,
        StackEvent::Duck {
            level: 1.0,
            duration_ms: 500
        }
    );
    let unduck = dict([("type", Value::String("unduckAudio".into()))]);
    rig.phone.plist("POST", "/command", &unduck).await;
    assert_eq!(
        rig.event().await,
        StackEvent::Duck {
            level: 1.0,
            duration_ms: 0
        }
    );
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
    for cmd in [
        "requestUI",
        "disableBluetooth",
        "suggestUI",
        "iAPSendMessage",
        "mystery",
    ] {
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
    assert_eq!(
        rig.event().await,
        StackEvent::DisableBluetooth("AA:BB".into())
    );

    rig.cmds
        .send(StackCmd::Touches(vec![Touch {
            x: 0.5,
            y: 0.5,
            down: true,
        }]))
        .unwrap();
    rig.cmds.send(StackCmd::Keyframe).unwrap();
    rig.cmds.send(StackCmd::NightMode(true)).unwrap();
    rig.cmds.send(StackCmd::ClusterNightMode(true)).unwrap();
    rig.cmds.send(StackCmd::LimitedUi(true)).unwrap();
    rig.cmds.send(StackCmd::Siri).unwrap();
    rig.cmds
        .send(StackCmd::Media(hid::media_button::NEXT))
        .unwrap();
    rig.cmds
        .send(StackCmd::Telephony(hid::telephony_button::DROP))
        .unwrap();
    rig.cmds
        .send(StackCmd::Knob {
            state: KnobState {
                x: 1.0,
                ..Default::default()
            },
            momentary: true,
        })
        .unwrap();
    rig.cmds.send(StackCmd::KnobSelect(true)).unwrap();
    rig.cmds.send(StackCmd::ClusterActive(true)).unwrap();
    rig.cmds
        .send(StackCmd::StreamVolume {
            kind: AudioKind::Call,
            level: 0.5,
            ramp_ms: 10,
        })
        .unwrap();
    let mut sealed = Vec::new();
    let mut plain = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    let mut commands = Vec::new();
    while commands.len() < 15 {
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
            commands.push(
                body.get("type")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_string(),
            );
        }
    }
    assert_eq!(
        commands,
        [
            "hidSendReport",
            "forceKeyFrame",
            "setNightMode",
            "uiAppearanceUpdate",
            "mapAppearanceUpdate",
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

    let ask = event_cipher.encrypt(b"POST /ping RTSP/1.0\r\nCSeq: 1\r\nContent-Length: 0\r\n\r\n");
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
    assert_eq!(
        rig.event().await,
        StackEvent::AudioActive {
            profile: call,
            active: false
        }
    );
    assert_eq!(
        rig.event().await,
        StackEvent::MicActive {
            active: false,
            rate: 0,
            channels: 0
        }
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
        assert!(
            calls.iter().any(|c| c == expected),
            "{expected} missing from {calls:?}"
        );
    }
    assert!(
        rig.helper_lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "tunnel phone-1 AA:BB")
    );
}

#[tokio::test]
async fn stop_ends_the_session_quietly() {
    let mut rig = Rig::new().await;
    rig.cmds.send(StackCmd::Siri).unwrap();
    rig.cmds.send(StackCmd::Stop).unwrap();
    assert_eq!(rig.event().await, StackEvent::Closed { was_active: false });
}
