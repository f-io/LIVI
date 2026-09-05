// One phone connection: version and TLS handshakes, then the demux. Media on the
// AV channels goes to gst-host and is acked here, everything else is relayed to
// the main process over the session socket.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_host_proto::feed as feedproto;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::{Notify, mpsc};

use crate::av;
use crate::consts::*;
use livi_session_io::feed::FeedWriter;
use crate::frame::{self, FrameParser, FrameSplitter, RawFrame};
use livi_session_io::link;
use crate::tls::{Message, TlsEngine};

const NODE_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Idle limit until the session is running, like the main process had.
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
const VIDEO_BACKLOG: usize = 240;
const AUDIO_BACKLOG: usize = 64;
const READ_CHUNK: usize = 65536;

/// Removes the session socket once the session is gone.
struct SocketPath(String);

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The phone behind a session: a label for the logs and the main process, and the
/// address the TLS client is built for. USB sessions carry a placeholder address.
pub struct Peer {
    pub label: String,
    pub ip: IpAddr,
}

/// How a session ended, for the transport that carried it.
pub struct SessionEnd {
    /// The main process asked for the end, so the phone should stay put.
    pub closed_by_node: bool,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Version,
    Handshake,
    Running,
}

/// Where the media of each channel goes, as the main process configured it.
#[derive(Default)]
struct Sinks {
    feed: Option<(String, FeedWriter)>,
    /// Channel → plane id and codec byte.
    video: HashMap<u8, (u32, u8)>,
    /// Channel → stream id.
    audio: HashMap<u8, u32>,
    /// Media that arrived before its sink was known.
    backlog: HashMap<u8, VecDeque<(u64, Vec<u8>)>>,
}

impl Sinks {
    fn apply(&mut self, v: &serde_json::Value) {
        if let Some(path) = v.get("feed").and_then(|p| p.as_str())
            && !path.is_empty()
            && self.feed.as_ref().is_none_or(|(p, _)| p != path)
        {
            self.feed = Some((path.to_owned(), FeedWriter::open(path.to_owned())));
            let known: Vec<(u8, u32, u8)> =
                self.video.iter().map(|(ch, (id, codec))| (*ch, *id, *codec)).collect();
            for (_, id, codec) in known {
                self.send(feedproto::encode(feedproto::KIND_VIDEO_START, id, 0, &[codec]));
            }
            let waiting: Vec<u8> = self.backlog.keys().copied().collect();
            for ch in waiting {
                self.flush(ch);
            }
        }
        for entry in v.get("video").and_then(|a| a.as_array()).into_iter().flatten() {
            let (Some(ch), Some(id)) = (entry.get("ch").and_then(|c| c.as_u64()), entry.get("id").and_then(|i| i.as_u64())) else {
                continue;
            };
            let codec = match entry.get("codec").and_then(|c| c.as_str()) {
                Some("h264") => 0,
                Some("h265") => 1,
                _ => 0xff,
            };
            self.video.insert(ch as u8, (id as u32, codec));
            self.send(feedproto::encode(feedproto::KIND_VIDEO_START, id as u32, 0, &[codec]));
            self.flush(ch as u8);
        }
        for entry in v.get("audio").and_then(|a| a.as_array()).into_iter().flatten() {
            let (Some(ch), Some(id)) = (entry.get("ch").and_then(|c| c.as_u64()), entry.get("id").and_then(|i| i.as_u64())) else {
                continue;
            };
            self.audio.insert(ch as u8, id as u32);
            self.flush(ch as u8);
        }
    }

    /// Media waits in the backlog until both the feed and the channel's sink are known.
    fn push(&mut self, ch: u8, ts: u64, data: &[u8]) {
        if self.feed.is_some()
            && let Some((kind, id)) = self.target(ch)
        {
            self.send(feedproto::encode(kind, id, ts, data));
            return;
        }
        let limit = if is_video_channel(ch) { VIDEO_BACKLOG } else { AUDIO_BACKLOG };
        let q = self.backlog.entry(ch).or_default();
        if q.len() >= limit {
            q.pop_front();
        }
        q.push_back((ts, data.to_vec()));
    }

    fn flush(&mut self, ch: u8) {
        if self.feed.is_none() {
            return;
        }
        let Some((kind, id)) = self.target(ch) else { return };
        let Some(q) = self.backlog.remove(&ch) else { return };
        for (ts, data) in q {
            self.send(feedproto::encode(kind, id, ts, &data));
        }
    }

    fn target(&self, ch: u8) -> Option<(u8, u32)> {
        if let Some((id, _)) = self.video.get(&ch) {
            return Some((feedproto::KIND_VIDEO, *id));
        }
        self.audio.get(&ch).map(|id| (feedproto::KIND_AUDIO, *id))
    }

    fn send(&self, record: Vec<u8>) {
        if let Some((_, w)) = &self.feed {
            w.send(record);
        }
    }
}

struct Session<W> {
    peer: Peer,
    /// Where the microphone tap delivers, told to the main process at ready.
    mic_path: String,
    wr: W,
    node_tx: mpsc::Sender<Vec<u8>>,
    phase: Phase,
    parser: FrameParser,
    splitter: FrameSplitter,
    tls: Option<TlsEngine>,
    sinks: Sinks,
    /// Channel → session id from its START_INDICATION, for the acks.
    session_ids: HashMap<u8, u32>,
    announced: HashSet<u8>,
    closed_sent: bool,
    closed_by_node: bool,
    video_frames: u64,
    audio_frames: u64,
}

pub async fn run<T>(
    io: T,
    peer: Peer,
    node: UnixListener,
    node_path: String,
    cancel: Arc<Notify>,
) -> SessionEnd
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let _path = SocketPath(node_path.clone());
    let node_stream = match tokio::time::timeout(NODE_ACCEPT_TIMEOUT, node.accept()).await {
        Ok(Ok((s, _))) => s,
        _ => {
            eprintln!("[aa-session] {}: main process did not attach to {node_path}", peer.label);
            return SessionEnd { closed_by_node: false };
        }
    };
    drop(node);

    // The microphone tap the main process opens in the pipeline connects here.
    let mic_path = format!("{node_path}.mic");
    let _mic_path = SocketPath(mic_path.clone());
    let (mic_tx, mut mic_rx) = mpsc::channel::<(u64, Vec<u8>)>(64);
    match UnixListener::bind(&mic_path) {
        Ok(listener) => {
            let _ = std::fs::set_permissions(&mic_path, std::fs::Permissions::from_mode(0o666));
            tokio::spawn(listen_mic(listener, mic_tx));
        }
        Err(e) => eprintln!("[aa-session] {}: mic socket {mic_path}: {e}", peer.label),
    }

    let (mut node_rd, mut node_wr) = node_stream.into_split();
    let (node_tx, mut node_rx) = mpsc::channel::<Vec<u8>>(1024);
    tokio::spawn(async move {
        while let Some(bytes) = node_rx.recv().await {
            if node_wr.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });
    let (item_tx, mut items) = mpsc::channel::<link::Item>(1024);
    tokio::spawn(async move {
        let mut framer = link::Framer::default();
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            let n = match node_rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&buf[..n]);
            while let Some(item) = framer.next_item() {
                if item_tx.send(item).await.is_err() {
                    return;
                }
            }
        }
    });

    let (mut rd, wr) = tokio::io::split(io);
    let mut s = Session {
        peer,
        mic_path,
        wr,
        node_tx,
        phase: Phase::Version,
        parser: FrameParser::default(),
        splitter: FrameSplitter::default(),
        tls: None,
        sinks: Sinks::default(),
        session_ids: HashMap::new(),
        announced: HashSet::new(),
        closed_sent: false,
        closed_by_node: false,
        video_frames: 0,
        audio_frames: 0,
    };

    if let Err(e) = s.send_version_request().await {
        s.closed(&e).await;
        return SessionEnd { closed_by_node: false };
    }

    let mut buf = vec![0u8; READ_CHUNK];
    let mut phone_open = true;
    loop {
        let setting_up = s.phase != Phase::Running;
        tokio::select! {
            r = rd.read(&mut buf), if phone_open => match r {
                Ok(0) => {
                    phone_open = false;
                    if !s.control("{\"type\":\"eof\"}").await {
                        break;
                    }
                }
                Ok(n) => {
                    if let Err(e) = s.on_tcp(&buf[..n]).await {
                        s.closed(&e).await;
                        break;
                    }
                }
                Err(e) => {
                    s.closed(&e.to_string()).await;
                    break;
                }
            },
            item = items.recv() => match item {
                Some(link::Item::Message { ch, flags, msg_id, payload }) => {
                    if let Err(e) = s.on_node_message(ch, flags, msg_id, &payload).await {
                        s.closed(&e).await;
                        break;
                    }
                }
                Some(link::Item::Control(json)) => {
                    if !s.on_control(&json).await {
                        break;
                    }
                }
                None => break,
            },
            mic = mic_rx.recv() => {
                if let Some((ts, pcm)) = mic
                    && let Err(e) = s.send_mic(ts, &pcm).await
                {
                    s.closed(&e).await;
                    break;
                }
            }
            _ = tokio::time::sleep(SETUP_TIMEOUT), if setting_up => {
                s.closed("setup timeout").await;
                break;
            }
            _ = cancel.notified() => {
                s.closed("phone unplugged").await;
                break;
            }
        }
    }
    println!(
        "[aa-session] {}: session over, video={} audio={}",
        s.peer.label, s.video_frames, s.audio_frames
    );
    SessionEnd { closed_by_node: s.closed_by_node }
}

impl<W: AsyncWrite + Unpin + Send> Session<W> {
    async fn send_version_request(&mut self) -> Result<(), String> {
        let mut data = VERSION_MAJOR.to_be_bytes().to_vec();
        data.extend_from_slice(&VERSION_MINOR.to_be_bytes());
        self.write(&frame::encode(CH_CONTROL, FLAGS_PLAINTEXT, CTRL_VERSION_REQUEST, &data)).await
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.wr.write_all(bytes).await.map_err(|e| format!("write: {e}"))
    }

    /// False once the main process is gone.
    async fn control(&mut self, json: &str) -> bool {
        self.node_tx.send(link::encode_control(json)).await.is_ok()
    }

    async fn forward(&mut self, ch: u8, flags: u8, msg_id: u16, payload: &[u8]) -> Result<(), String> {
        self.node_tx
            .send(link::encode_message(ch, flags, msg_id, payload))
            .await
            .map_err(|_| "main process gone".to_owned())
    }

    async fn closed(&mut self, reason: &str) {
        if self.closed_sent {
            return;
        }
        self.closed_sent = true;
        println!("[aa-session] {}: closed ({reason})", self.peer.label);
        let json = serde_json::json!({ "type": "closed", "reason": reason }).to_string();
        self.control(&json).await;
    }

    async fn on_tcp(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.phase == Phase::Running {
            self.splitter.push(bytes);
        } else {
            self.parser.push(bytes);
        }
        loop {
            if self.phase == Phase::Running {
                let Some(f) = self.splitter.next_frame() else { break };
                self.on_frame(f).await?;
            } else {
                let Some(f) = self.parser.next_frame() else { break };
                self.on_setup_frame(f).await?;
            }
        }
        Ok(())
    }

    async fn on_setup_frame(&mut self, f: RawFrame) -> Result<(), String> {
        if f.flags & FLAG_ENCRYPTED != 0 {
            // an encrypted frame riding on the segment that finished the handshake
            return self.on_encrypted(f).await;
        }
        let Some((msg_id, body)) = f.message() else { return Ok(()) };
        match (msg_id, &self.phase) {
            (CTRL_VERSION_RESPONSE, Phase::Version) => {
                if body.len() < 6 {
                    return Err("version response too short".into());
                }
                let status = u16::from_be_bytes([body[4], body[5]]);
                if status == VERSION_STATUS_MISMATCH {
                    return Err(format!("version mismatch {}.{}", u16::from_be_bytes([body[0], body[1]]), u16::from_be_bytes([body[2], body[3]])));
                }
                let mut tls = TlsEngine::new(self.peer.ip).map_err(|e| e.to_string())?;
                let hello = tls.take_output();
                self.tls = Some(tls);
                self.phase = Phase::Handshake;
                self.write(&frame::encode(CH_CONTROL, FLAGS_PLAINTEXT, CTRL_SSL_HANDSHAKE, &hello)).await
            }
            (CTRL_SSL_HANDSHAKE, Phase::Handshake) => {
                let body = body.to_vec();
                let tls = self.tls.as_mut().ok_or("no tls")?;
                tls.inject_handshake(&body).map_err(|e| e.to_string())?;
                let out = tls.take_output();
                let done = !tls.is_handshaking();
                if !out.is_empty() {
                    self.write(&frame::encode(CH_CONTROL, FLAGS_PLAINTEXT, CTRL_SSL_HANDSHAKE, &out)).await?;
                }
                if done {
                    self.become_running().await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn become_running(&mut self) -> Result<(), String> {
        self.splitter = std::mem::take(&mut self.parser).into_splitter();
        self.phase = Phase::Running;
        println!("[aa-session] {}: tls up", self.peer.label);
        let json =
            serde_json::json!({ "type": "ready", "peer": self.peer.label, "mic": self.mic_path })
                .to_string();
        if !self.control(&json).await {
            return Err("main process gone".into());
        }
        Ok(())
    }

    async fn on_frame(&mut self, f: RawFrame) -> Result<(), String> {
        if f.flags & FLAG_ENCRYPTED == 0 {
            if let Some((msg_id, body)) = f.message() {
                let body = body.to_vec();
                self.forward(f.ch, f.flags, msg_id, &body).await?;
            }
            return Ok(());
        }
        self.on_encrypted(f).await
    }

    async fn on_encrypted(&mut self, f: RawFrame) -> Result<(), String> {
        let tls = self.tls.as_mut().ok_or("encrypted frame before tls")?;
        let messages = tls.inject_record(f.ch, f.flags, &f.payload).map_err(|e| e.to_string())?;
        for m in messages {
            self.dispatch(m).await?;
        }
        Ok(())
    }

    async fn dispatch(&mut self, m: Message) -> Result<(), String> {
        if is_video_channel(m.ch) || is_audio_channel(m.ch) {
            if is_media_message(m.msg_id) {
                return self.media(m).await;
            }
            if m.msg_id == AV_START_INDICATION {
                self.session_ids.insert(m.ch, av::start_session_id(&m.payload).unwrap_or(0));
            }
        }
        self.forward(m.ch, m.flags, m.msg_id, &m.payload).await
    }

    async fn media(&mut self, m: Message) -> Result<(), String> {
        let session_id = self.session_ids.get(&m.ch).copied().unwrap_or(0);
        let ack = self
            .tls
            .as_mut()
            .ok_or("no tls")?
            .encrypt(m.ch, FLAGS_ENC_SIGNAL, AV_MEDIA_ACK, &av::ack(session_id))
            .map_err(|e| e.to_string())?;
        self.write(&ack).await?;

        let (ts, data) = av::media(m.msg_id, &m.payload);
        let ts = ts.unwrap_or_else(now_ns);
        if is_video_channel(m.ch) {
            self.video_frames += 1;
        } else {
            self.audio_frames += 1;
        }
        self.sinks.push(m.ch, ts, data);

        if self.announced.insert(m.ch) {
            let json = serde_json::json!({ "type": "first-frame", "ch": m.ch }).to_string();
            if !self.control(&json).await {
                return Err("main process gone".into());
            }
        }
        Ok(())
    }

    /// Captured samples go to the phone's microphone channel, stamped in microseconds.
    async fn send_mic(&mut self, ts_ns: u64, pcm: &[u8]) -> Result<(), String> {
        if self.phase != Phase::Running || pcm.is_empty() {
            return Ok(());
        }
        let mut payload = (ts_ns / 1000).to_be_bytes().to_vec();
        payload.extend_from_slice(pcm);
        let wire = self
            .tls
            .as_mut()
            .ok_or("no tls")?
            .encrypt(CH_MIC_INPUT, FLAGS_ENC_SIGNAL, AV_MEDIA_WITH_TIMESTAMP, &payload)
            .map_err(|e| e.to_string())?;
        self.write(&wire).await
    }

    async fn on_node_message(&mut self, ch: u8, flags: u8, msg_id: u16, payload: &[u8]) -> Result<(), String> {
        let wire = if flags & FLAG_ENCRYPTED != 0 {
            let Some(tls) = self.tls.as_mut() else {
                eprintln!("[aa-session] {}: encrypted message before tls, dropped", self.peer.label);
                return Ok(());
            };
            tls.encrypt(ch, flags, msg_id, payload).map_err(|e| e.to_string())?
        } else {
            frame::encode(ch, flags, msg_id, payload)
        };
        self.write(&wire).await
    }

    /// False when the main process asked for the end of the session.
    async fn on_control(&mut self, json: &str) -> bool {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return true };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("sink") => self.sinks.apply(&v),
            // A session the main process ends before it ran is a failed bring-up,
            // which the transport may retry.
            Some("end") => {
                self.closed_by_node = self.phase == Phase::Running;
                let _ = self.wr.shutdown().await;
            }
            Some("close") => {
                self.closed_by_node = self.phase == Phase::Running;
                return false;
            }
            _ => {}
        }
        true
    }
}

/// Takes the microphone records the pipeline streams in, one connection at a time.
async fn listen_mic(listener: UnixListener, tx: mpsc::Sender<(u64, Vec<u8>)>) {
    while let Ok((mut sock, _)) = listener.accept().await {
        let mut framer = feedproto::Framer::new();
        let mut buf = vec![0u8; 16384];
        loop {
            let n = match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&buf[..n]);
            while let Some(record) = framer.next_record() {
                // a slow session drops samples rather than piling them up
                if record.kind == feedproto::KIND_MIC && tx.try_send((record.ts, record.payload)).is_err() {
                    continue;
                }
            }
        }
    }
}

fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}
