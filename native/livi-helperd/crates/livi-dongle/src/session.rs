// One dongle session. Its messages reach the main process over the session socket,
// video and audio go to the pipeline's feed, the microphone comes back from the
// pipeline's tap. The heartbeat and the keyframe watchdog run here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_aa::feed::FeedWriter;
use livi_aa::link::{self, Item};
use livi_host_proto::feed as feedproto;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio::sync::Notify;

use crate::wire;

const NODE_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_secs(2);
const VIDEO_BACKLOG: usize = 240;
const AUDIO_BACKLOG: usize = 64;
const READ_CHUNK: usize = 65536;
/// The microphone goes out in the dongle's phone microphone slot.
const MIC_AUDIO_TYPE: u32 = 3;

pub struct Ended {
    /// The main process closed the session, the dongle itself is still there.
    pub by_node: bool,
    /// The main process asked for a USB reset of the dongle.
    pub reset: bool,
}

/// Removes the socket once the session is gone.
struct SocketPath(String);

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Video { cluster: bool },
    Audio { decode: u32, kind: u32 },
}

/// Where the media goes, as the main process configured it.
#[derive(Default)]
struct Sinks {
    feed: Option<(String, FeedWriter)>,
    /// Cluster flag → plane id and codec byte.
    video: HashMap<bool, (u32, u8)>,
    /// Decode type and audio type → stream id.
    audio: HashMap<(u32, u32), u32>,
    /// Media that arrived before its sink was known.
    backlog: HashMap<Key, VecDeque<(u64, Vec<u8>)>>,
}

impl Sinks {
    fn apply(&mut self, v: &serde_json::Value) {
        if let Some(path) = v.get("feed").and_then(|p| p.as_str())
            && !path.is_empty()
            && self.feed.as_ref().is_none_or(|(p, _)| p != path)
        {
            self.feed = Some((path.to_owned(), FeedWriter::open(path.to_owned())));
            let known: Vec<(u32, u8)> = self.video.values().copied().collect();
            for (id, codec) in known {
                self.send(feedproto::encode(feedproto::KIND_VIDEO_START, id, 0, &[codec]));
            }
            let waiting: Vec<Key> = self.backlog.keys().copied().collect();
            for key in waiting {
                self.flush(key);
            }
        }
        for entry in v.get("video").and_then(|a| a.as_array()).into_iter().flatten() {
            let Some(id) = entry.get("id").and_then(|i| i.as_u64()) else { continue };
            let cluster = entry.get("cluster").and_then(|c| c.as_bool()).unwrap_or(false);
            let codec = match entry.get("codec").and_then(|c| c.as_str()) {
                Some("h265") => 1,
                _ => 0,
            };
            self.video.insert(cluster, (id as u32, codec));
            self.send(feedproto::encode(feedproto::KIND_VIDEO_START, id as u32, 0, &[codec]));
            self.flush(Key::Video { cluster });
        }
        for entry in v.get("audio").and_then(|a| a.as_array()).into_iter().flatten() {
            let (Some(decode), Some(kind), Some(id)) = (
                entry.get("decodeType").and_then(|d| d.as_u64()),
                entry.get("audioType").and_then(|t| t.as_u64()),
                entry.get("id").and_then(|i| i.as_u64()),
            ) else {
                continue;
            };
            self.audio.insert((decode as u32, kind as u32), id as u32);
            self.flush(Key::Audio { decode: decode as u32, kind: kind as u32 });
        }
    }

    /// Media waits in the backlog until both the feed and its sink are known.
    fn push(&mut self, key: Key, ts: u64, data: &[u8]) {
        if self.feed.is_some()
            && let Some((kind, id)) = self.target(key)
        {
            self.send(feedproto::encode(kind, id, ts, data));
            return;
        }
        let limit = match key {
            Key::Video { .. } => VIDEO_BACKLOG,
            Key::Audio { .. } => AUDIO_BACKLOG,
        };
        let q = self.backlog.entry(key).or_default();
        if q.len() >= limit {
            q.pop_front();
        }
        q.push_back((ts, data.to_vec()));
    }

    fn flush(&mut self, key: Key) {
        if self.feed.is_none() {
            return;
        }
        let Some((kind, id)) = self.target(key) else { return };
        let Some(q) = self.backlog.remove(&key) else { return };
        for (ts, data) in q {
            self.send(feedproto::encode(kind, id, ts, &data));
        }
    }

    fn target(&self, key: Key) -> Option<(u8, u32)> {
        match key {
            Key::Video { cluster } => self.video.get(&cluster).map(|(id, _)| (feedproto::KIND_VIDEO, *id)),
            Key::Audio { decode, kind } => {
                self.audio.get(&(decode, kind)).map(|id| (feedproto::KIND_AUDIO, *id))
            }
        }
    }

    fn send(&self, record: Vec<u8>) {
        if let Some((_, w)) = &self.feed {
            w.send(record);
        }
    }
}

struct Session<W> {
    label: String,
    wr: W,
    node_tx: mpsc::Sender<Vec<u8>>,
    sinks: Sinks,
    /// Decode type of the microphone stream while the main process wants one.
    mic: Option<u32>,
    /// The dongle answered the open, heartbeats keep it awake from here.
    opened: bool,
    /// Width and height per stream, the main process hears about changes.
    geometry: HashMap<bool, (u32, u32)>,
    /// Audio formats the main process was told about.
    formats: HashSet<(u32, u32)>,
}

pub async fn run<T>(
    io: T,
    label: &str,
    node: UnixListener,
    node_path: String,
    cancel: Arc<Notify>,
) -> Ended
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let _path = SocketPath(node_path.clone());
    let node_stream = match tokio::time::timeout(NODE_ACCEPT_TIMEOUT, node.accept()).await {
        Ok(Ok((s, _))) => s,
        _ => {
            eprintln!("[dongle] {label}: main process did not attach to {node_path}");
            return Ended { by_node: false, reset: false };
        }
    };
    drop(node);

    // The microphone tap the main process opens in the pipeline connects here.
    let mic_path = format!("{node_path}.mic");
    let _ = std::fs::remove_file(&mic_path);
    let _mic_path = SocketPath(mic_path.clone());
    let (mic_tx, mut mic_rx) = mpsc::channel::<Vec<u8>>(64);
    match UnixListener::bind(&mic_path) {
        Ok(listener) => {
            let _ = std::fs::set_permissions(&mic_path, std::fs::Permissions::from_mode(0o666));
            tokio::spawn(listen_mic(listener, mic_tx));
        }
        Err(e) => eprintln!("[dongle] {label}: mic socket {mic_path}: {e}"),
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
    let (item_tx, mut items) = mpsc::channel::<Item>(1024);
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

    let (rd, wr) = tokio::io::split(io);
    let (usb_tx, mut usb) = mpsc::channel::<(u32, Vec<u8>)>(256);
    tokio::spawn(read_messages(rd, usb_tx, label.to_owned()));

    let mut s = Session {
        label: label.to_owned(),
        wr,
        node_tx,
        sinks: Sinks::default(),
        mic: None,
        opened: false,
        geometry: HashMap::new(),
        formats: HashSet::new(),
    };
    s.control(serde_json::json!({ "type": "ready", "mic": mic_path })).await;
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    let mut mic_open = true;
    loop {
        let sent = tokio::select! {
            msg = usb.recv() => match msg {
                Some((kind, payload)) => s.dongle_message(kind, payload).await,
                None => Err("dongle read ended".to_owned()),
            },
            item = items.recv() => match item {
                Some(Item::Message { msg_id, payload, .. }) => s.write_dongle(u32::from(msg_id), &payload).await,
                Some(Item::Control(json)) => match s.control_from_node(&json) {
                    Some(reset) => return Ended { by_node: true, reset },
                    None => Ok(()),
                },
                None => return Ended { by_node: true, reset: false },
            },
            pcm = mic_rx.recv(), if mic_open => match (pcm, s.mic) {
                (None, _) => {
                    mic_open = false;
                    Ok(())
                }
                (Some(pcm), Some(decode)) => s.send_mic(decode, &pcm).await,
                (Some(_), None) => Ok(()),
            },
            _ = heartbeat.tick(), if s.opened => s.write_dongle(wire::HEARTBEAT, &[]).await,
            _ = cancel.notified() => {
                s.closed("dongle unplugged").await;
                return Ended { by_node: false, reset: false };
            }
        };
        if let Err(e) = sent {
            s.closed(&e).await;
            return Ended { by_node: false, reset: false };
        }
    }
}

impl<W: AsyncWrite + Unpin> Session<W> {
    async fn dongle_message(&mut self, kind: u32, payload: Vec<u8>) -> Result<(), String> {
        let word = |i: usize| u32::from_le_bytes(payload[i..i + 4].try_into().unwrap());
        match kind {
            wire::VIDEO | wire::CLUSTER_VIDEO if payload.len() > wire::VIDEO_HEAD => {
                let cluster = kind == wire::CLUSTER_VIDEO;
                let size = (word(0), word(4));
                if self.geometry.insert(cluster, size) != Some(size) {
                    self.control(serde_json::json!({
                        "type": "video", "cluster": cluster, "width": size.0, "height": size.1
                    }))
                    .await;
                }
                self.sinks.push(Key::Video { cluster }, now_ns(), &payload[wire::VIDEO_HEAD..]);
                Ok(())
            }
            wire::AUDIO if is_pcm(&payload) => {
                let (decode, kind) = (word(0), word(8));
                if self.formats.insert((decode, kind)) {
                    self.control(serde_json::json!({
                        "type": "audio-setup", "decodeType": decode, "audioType": kind
                    }))
                    .await;
                }
                self.sinks.push(Key::Audio { decode, kind }, now_ns(), &payload[wire::AUDIO_HEAD..]);
                Ok(())
            }
            _ => {
                if kind == wire::OPEN && !payload.is_empty() {
                    self.opened = true;
                }
                if kind == wire::UNPLUGGED {
                    self.opened = false;
                    self.geometry.clear();
                    self.formats.clear();
                }
                let _ = self.node_tx.send(link::encode_message(0, 0, kind as u16, &payload)).await;
                Ok(())
            }
        }
    }

    async fn write_dongle(&mut self, kind: u32, payload: &[u8]) -> Result<(), String> {
        self.wr.write_all(&wire::message(kind, payload)).await.map_err(|e| format!("dongle write: {e}"))
    }

    async fn send_mic(&mut self, decode: u32, pcm: &[u8]) -> Result<(), String> {
        let mut payload = Vec::with_capacity(wire::AUDIO_HEAD + pcm.len());
        payload.extend_from_slice(&decode.to_le_bytes());
        payload.extend_from_slice(&0f32.to_le_bytes());
        payload.extend_from_slice(&MIC_AUDIO_TYPE.to_le_bytes());
        payload.extend_from_slice(pcm);
        self.write_dongle(wire::AUDIO, &payload).await
    }

    /// Some when the main process ends the session, with whether it wants a USB reset.
    fn control_from_node(&mut self, json: &str) -> Option<bool> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return None };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("sink") => self.sinks.apply(&v),
            Some("mic") => {
                self.mic = v.get("decodeType").and_then(|d| d.as_u64()).map(|d| d as u32);
                match self.mic {
                    Some(d) => println!("[dongle] {}: microphone on, decode type {d}", self.label),
                    None => println!("[dongle] {}: microphone off", self.label),
                }
            }
            Some("close") => return Some(false),
            Some("reset") => return Some(true),
            _ => {}
        }
        None
    }

    async fn control(&self, v: serde_json::Value) {
        let _ = self.node_tx.send(link::encode_control(&v.to_string())).await;
    }

    async fn closed(&self, reason: &str) {
        eprintln!("[dongle] {}: {reason}", self.label);
        self.control(serde_json::json!({ "type": "closed", "reason": reason })).await;
    }
}

/// Samples, as opposed to a one byte command or a four byte volume ramp.
fn is_pcm(payload: &[u8]) -> bool {
    let rest = payload.len().saturating_sub(wire::AUDIO_HEAD);
    payload.len() > wire::AUDIO_HEAD && rest != 1 && rest != 4
}

/// Frames the dongle's messages off the pipe, until it fails or the session is gone.
async fn read_messages<R: AsyncRead + Unpin>(mut rd: R, tx: mpsc::Sender<(u32, Vec<u8>)>, label: String) {
    loop {
        let mut head = [0u8; wire::HEADER_LEN];
        if let Err(e) = rd.read_exact(&mut head).await {
            if e.kind() != std::io::ErrorKind::UnexpectedEof {
                eprintln!("[dongle] {label}: read: {e}");
            }
            return;
        }
        let (kind, len) = match wire::parse_header(&head) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[dongle] {label}: {e}");
                return;
            }
        };
        let mut payload = vec![0u8; len];
        if let Err(e) = rd.read_exact(&mut payload).await {
            eprintln!("[dongle] {label}: read: {e}");
            return;
        }
        if tx.send((kind, payload)).await.is_err() {
            return;
        }
    }
}

/// Takes the microphone records the pipeline streams in, one connection at a time.
async fn listen_mic(listener: UnixListener, tx: mpsc::Sender<Vec<u8>>) {
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
                if record.kind == feedproto::KIND_MIC && tx.try_send(record.payload).is_err() {
                    continue;
                }
            }
        }
    }
}

fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::DuplexStream;
    use tokio::net::UnixStream;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn sock_path(stem: &str) -> String {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("livi-dongle-{stem}-{}-{n}.sock", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    struct Bench {
        cancel: std::sync::Arc<tokio::sync::Notify>,
        /// The dongle's end of the pipe.
        usb: DuplexStream,
        node: UnixStream,
        node_path: String,
        session: tokio::task::JoinHandle<Ended>,
        framer: link::Framer,
    }

    async fn bench() -> Bench {
        let (usb, io) = tokio::io::duplex(1 << 20);
        let node_path = sock_path("session");
        let listener = UnixListener::bind(&node_path).unwrap();
        let path = node_path.clone();
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        let run_cancel = cancel.clone();
        let session = tokio::spawn(async move { run(io, "test", listener, path, run_cancel).await });
        let node = UnixStream::connect(&node_path).await.unwrap();
        let mut b = Bench { cancel, usb, node, node_path, session, framer: link::Framer::default() };
        assert_eq!(b.next_item().await, Item::Control("{\"mic\":\"".to_owned() + &b.node_path + ".mic\",\"type\":\"ready\"}"));
        b
    }

    impl Bench {
        async fn next_item(&mut self) -> Item {
            loop {
                if let Some(item) = self.framer.next_item() {
                    return item;
                }
                let mut buf = [0u8; 4096];
                let n = self.node.read(&mut buf).await.unwrap();
                assert!(n > 0, "session socket closed");
                self.framer.push(&buf[..n]);
            }
        }

        async fn dongle_sends(&mut self, kind: u32, payload: &[u8]) {
            self.usb.write_all(&wire::message(kind, payload)).await.unwrap();
        }

        async fn at_dongle(&mut self) -> (u32, Vec<u8>) {
            let mut head = [0u8; wire::HEADER_LEN];
            self.usb.read_exact(&mut head).await.unwrap();
            let (kind, len) = wire::parse_header(&head).unwrap();
            let mut payload = vec![0u8; len];
            self.usb.read_exact(&mut payload).await.unwrap();
            (kind, payload)
        }

        async fn control(&mut self, json: &str) {
            self.node.write_all(&link::encode_control(json)).await.unwrap();
        }
    }

    async fn feed_records(feed: &mut UnixStream, count: usize) -> Vec<feedproto::Record> {
        let mut framer = feedproto::Framer::new();
        let mut out = Vec::new();
        let mut buf = [0u8; 65536];
        while out.len() < count {
            let n = feed.read(&mut buf).await.unwrap();
            assert!(n > 0, "feed closed");
            framer.push(&buf[..n]);
            while let Some(r) = framer.next_record() {
                out.push(r);
            }
        }
        out
    }

    #[tokio::test]
    async fn messages_pass_both_ways_and_the_main_process_can_close() {
        let mut b = bench().await;
        b.dongle_sends(0x02, &[3, 0, 0, 0]).await;
        assert_eq!(b.next_item().await, Item::Message { ch: 0, flags: 0, msg_id: 2, payload: vec![3, 0, 0, 0] });
        b.node.write_all(&link::encode_message(0, 0, 0x08, &[1, 0, 0, 0])).await.unwrap();
        assert_eq!(b.at_dongle().await, (0x08, vec![1, 0, 0, 0]));
        b.control("{\"type\":\"close\"}").await;
        assert!(b.session.await.unwrap().by_node);
        assert!(!std::path::Path::new(&b.node_path).exists());
    }

    #[tokio::test]
    async fn video_reaches_the_feed_once_its_plane_is_known() {
        let mut b = bench().await;
        let feed_path = sock_path("feed");
        let feed = UnixListener::bind(&feed_path).unwrap();
        let mut frame = Vec::new();
        for v in [800u32, 480, 0, 5, 0] {
            frame.extend_from_slice(&v.to_le_bytes());
        }
        frame.extend_from_slice(&[0, 0, 0, 1, 0x65]);
        b.dongle_sends(wire::VIDEO, &frame).await;
        assert_eq!(
            b.next_item().await,
            Item::Control("{\"cluster\":false,\"height\":480,\"type\":\"video\",\"width\":800}".into())
        );
        b.control(&format!("{{\"type\":\"sink\",\"feed\":\"{feed_path}\",\"video\":[{{\"cluster\":false,\"id\":7,\"codec\":\"h264\"}}]}}"))
            .await;
        let (mut conn, _) = feed.accept().await.unwrap();
        let records = feed_records(&mut conn, 2).await;
        assert_eq!((records[0].kind, records[0].id, &records[0].payload[..]), (feedproto::KIND_VIDEO_START, 7, &[0][..]));
        assert_eq!((records[1].kind, records[1].id, &records[1].payload[..]), (feedproto::KIND_VIDEO, 7, &[0, 0, 0, 1, 0x65][..]));
        b.control("{\"type\":\"close\"}").await;
        b.session.await.unwrap();
        let _ = std::fs::remove_file(&feed_path);
    }

    #[tokio::test]
    async fn audio_announces_its_format_and_commands_go_to_the_main_process() {
        let mut b = bench().await;
        let feed_path = sock_path("feed");
        let feed = UnixListener::bind(&feed_path).unwrap();
        b.control(&format!("{{\"type\":\"sink\",\"feed\":\"{feed_path}\"}}")).await;
        let (mut conn, _) = feed.accept().await.unwrap();
        let mut head = Vec::new();
        head.extend_from_slice(&5u32.to_le_bytes());
        head.extend_from_slice(&0f32.to_le_bytes());
        head.extend_from_slice(&1u32.to_le_bytes());
        let mut command = head.clone();
        command.push(10);
        b.dongle_sends(wire::AUDIO, &command).await;
        assert_eq!(b.next_item().await, Item::Message { ch: 0, flags: 0, msg_id: 7, payload: command });
        let mut pcm = head.clone();
        pcm.extend_from_slice(&[1, 0, 2, 0, 3, 0]);
        b.dongle_sends(wire::AUDIO, &pcm).await;
        assert_eq!(b.next_item().await, Item::Control("{\"audioType\":1,\"decodeType\":5,\"type\":\"audio-setup\"}".into()));
        b.control("{\"type\":\"sink\",\"audio\":[{\"decodeType\":5,\"audioType\":1,\"id\":9}]}").await;
        let records = feed_records(&mut conn, 1).await;
        assert_eq!((records[0].kind, records[0].id, &records[0].payload[..]), (feedproto::KIND_AUDIO, 9, &[1, 0, 2, 0, 3, 0][..]));
        b.control("{\"type\":\"close\"}").await;
        b.session.await.unwrap();
        let _ = std::fs::remove_file(&feed_path);
    }

    #[tokio::test]
    async fn the_tap_becomes_dongle_audio_while_the_microphone_is_on() {
        let mut b = bench().await;
        let mic_path = format!("{}.mic", b.node_path);
        let mut tap = UnixStream::connect(&mic_path).await.unwrap();
        b.control("{\"type\":\"mic\",\"decodeType\":5}").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        tap.write_all(&feedproto::encode(feedproto::KIND_MIC, 0, 2, &[7, 0, 8, 0])).await.unwrap();
        let (kind, payload) = b.at_dongle().await;
        assert_eq!(kind, wire::AUDIO);
        let mut want = Vec::new();
        want.extend_from_slice(&5u32.to_le_bytes());
        want.extend_from_slice(&0f32.to_le_bytes());
        want.extend_from_slice(&3u32.to_le_bytes());
        want.extend_from_slice(&[7, 0, 8, 0]);
        assert_eq!(payload, want);
        b.control("{\"type\":\"close\"}").await;
        b.session.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_follow_the_open_reply() {
        let mut b = bench().await;
        b.dongle_sends(wire::OPEN, &[0; 28]).await;
        assert_eq!(b.next_item().await, Item::Message { ch: 0, flags: 0, msg_id: 1, payload: vec![0; 28] });
        assert_eq!(b.at_dongle().await, (wire::HEARTBEAT, vec![]));
        b.control("{\"type\":\"close\"}").await;
        b.session.await.unwrap();
    }

    #[tokio::test]
    async fn a_reset_control_ends_the_session_with_a_reset() {
        let mut b = bench().await;
        b.control("{\"type\":\"reset\"}").await;
        let end = b.session.await.unwrap();
        assert!(end.by_node && end.reset);
    }

    #[tokio::test]
    async fn an_unplug_signal_ends_the_session_with_closed() {
        let mut b = bench().await;
        b.cancel.notify_waiters();
        assert_eq!(
            b.next_item().await,
            Item::Control("{\"reason\":\"dongle unplugged\",\"type\":\"closed\"}".into())
        );
        assert!(!b.session.await.unwrap().by_node);
    }

    #[tokio::test]
    async fn a_dongle_that_stops_talking_ends_the_session() {
        let mut b = bench().await;
        drop(std::mem::replace(&mut b.usb, tokio::io::duplex(1).0));
        assert_eq!(b.next_item().await, Item::Control("{\"reason\":\"dongle read ended\",\"type\":\"closed\"}".into()));
        assert!(!b.session.await.unwrap().by_node);
    }
}
