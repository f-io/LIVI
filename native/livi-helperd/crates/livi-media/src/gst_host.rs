use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use livi_host_proto::{Framer, encode_reply as frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, mpsc, oneshot};

const OP_CREATE: u8 = 1;
const OP_STOP: u8 = 3;
const OP_LISTEN: u8 = 5;
const OP_TEARDOWN: u8 = 6;
const OP_SET_ACTIVE: u8 = 7;
const OP_AUDIO_OPEN: u8 = 8;
const OP_AUDIO_VOLUME: u8 = 9;
const OP_AUDIO_STOP: u8 = 10;
const OP_MIC_OPEN: u8 = 11;
const OP_MIC_STOP: u8 = 12;
const OP_AUDIO_ACTIVE: u8 = 13;
const OP_AUDIO_DATA: u8 = 14;
const OP_VISUALIZER: u8 = 15;
const OP_FEED_OPEN: u8 = 16;
const OP_AUDIO_OUTPUT: u8 = 17;
const OP_FEED_ROUTE: u8 = 18;
const OP_FEED_CLOSE: u8 = 19;
const OP_TAP_OPEN: u8 = 20;
const OP_TAP_STOP: u8 = 21;
const OP_PLAY_URL: u8 = 22;
const OP_PLAY_RATE: u8 = 23;
const OP_PLAY_SEEK: u8 = 24;
const OP_PLAY_MUTE: u8 = 25;

const REPLY_PORT: u8 = 1;
const REPLY_CONFIG: u8 = 2;
const REPLY_STARTED: u8 = 3;
const REPLY_AUDIO_PORTS: u8 = 4;
const REPLY_AUDIO_STARTED: u8 = 5;
const REPLY_VISUALIZER: u8 = 6;
const REPLY_FEED: u8 = 7;
const REPLY_URL_STATUS: u8 = 8;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    Vp9,
    Av1,
}

impl Codec {
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
            Codec::Vp9 => "vp9",
            Codec::Av1 => "av1",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    Config {
        plane: u32,
        codec: Codec,
        atom: Vec<u8>,
    },
    Started {
        plane: u32,
    },
    AudioStarted {
        stream: u32,
        first_sample: u32,
    },
    /// Every stream summed, tapped before the fader, 0 to 1 per band.
    Spectrum {
        bands: Vec<f32>,
    },
    /// Where a plane that plays a URL stands, twice a second.
    UrlStatus {
        plane: u32,
        status: livi_host_proto::UrlStatus,
    },
}

/// Where a fed stream of its own ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedTarget {
    Plane(u32),
    Audio(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    AacLc = 0,
    Opus = 1,
    Pcm = 2,
    PcmLe = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOpts {
    pub codec: AudioCodec,
    /// The RTP payload type, which is the CarPlay stream type.
    pub payload_type: u8,
    pub clock_rate: u32,
    pub channels: u8,
    pub latency_ms: u32,
    /// Voice and call streams take the short path to the sink.
    pub realtime: bool,
    /// We push the samples, the host binds no ports.
    pub fed: bool,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicOpts {
    pub pcm: bool,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub frame_ms: u32,
    /// Where the phone listens.
    pub port: u16,
    pub phone: String,
    pub device: String,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub bin: PathBuf,
    pub env: Vec<(String, String)>,
    pub log: PathBuf,
    pub crash: PathBuf,
}

#[derive(Default)]
struct State {
    out: Option<(u64, mpsc::UnboundedSender<Vec<u8>>)>,
    queue: Vec<Vec<u8>>,
    running: bool,
    unavailable: bool,
    port_waiters: HashMap<u32, oneshot::Sender<u16>>,
    audio_waiters: HashMap<u32, oneshot::Sender<(u16, u16)>>,
    feed_waiter: Option<oneshot::Sender<String>>,
    feed_path: Option<String>,
    accept_task: Option<tokio::task::JoinHandle<()>>,
    /// Told to every host that connects, one that started again knows nothing.
    tap: bool,
}

struct Inner {
    sock_path: PathBuf,
    feed_path: PathBuf,
    launch: Mutex<Option<Launch>>,
    state: Mutex<State>,
    events: broadcast::Sender<HostEvent>,
    next_id: AtomicU32,
    next_conn: AtomicU64,
    feed_lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct GstHost {
    inner: Arc<Inner>,
}

impl GstHost {
    pub fn new(sock_path: PathBuf) -> Self {
        let feed_path = PathBuf::from(format!("{}.feed", sock_path.display()));
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                sock_path,
                feed_path,
                launch: Mutex::new(None),
                state: Mutex::new(State::default()),
                events,
                next_id: AtomicU32::new(0x7b00_0000),
                next_conn: AtomicU64::new(0),
                feed_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Used the next time the host starts.
    pub fn set_launch(&self, launch: Launch) {
        *self.inner.launch.lock().unwrap_or_else(|e| e.into_inner()) = Some(launch);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HostEvent> {
        self.inner.events.subscribe()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn next_id(&self) -> u32 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn start(&self) {
        let launch = self.inner.launch.lock().unwrap_or_else(|e| e.into_inner()).clone();
        {
            let mut st = self.state();
            if st.running || st.unavailable {
                return;
            }
            st.running = true;
        }
        let _ = std::fs::remove_file(&self.inner.sock_path);
        let listener = match UnixListener::bind(&self.inner.sock_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[core] gst-host socket: {e}");
                self.state().running = false;
                return;
            }
        };
        let accept = tokio::spawn(self.clone().accept(listener));
        self.state().accept_task = Some(accept);
        if let Some(launch) = launch {
            self.spawn_host(launch);
        }
    }

    fn spawn_host(&self, launch: Launch) {
        let _ = std::fs::remove_file(&launch.crash);
        {
            use std::os::unix::fs::PermissionsExt;
            // The packaged copy can lose its exec bit.
            let _ = std::fs::set_permissions(&launch.bin, std::fs::Permissions::from_mode(0o755));
        }
        let (stdout, stderr) = match log_file(&launch.log) {
            Some(f) => (
                f.try_clone().map(Stdio::from).unwrap_or_else(|_| Stdio::inherit()),
                Stdio::from(f),
            ),
            None => (Stdio::inherit(), Stdio::inherit()),
        };
        let child = tokio::process::Command::new(&launch.bin)
            .arg(&self.inner.sock_path)
            .arg(&launch.crash)
            .envs(launch.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[core] cannot start gst-host, media has nowhere to go: {e}");
                let mut st = self.state();
                st.unavailable = true;
                self.reset(&mut st);
                return;
            }
        };
        let host = self.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            eprintln!("[core] gst-host exited: {status:?}");
            if let Ok(st) = &status {
                use std::os::unix::process::ExitStatusExt;
                if st.signal().is_some()
                    && let Ok(trace) = std::fs::read_to_string(&launch.crash)
                {
                    eprintln!(
                        "[core] gst-host crash backtrace ({}):\n{trace}",
                        launch.crash.display()
                    );
                }
            }
            let mut st = host.state();
            host.reset(&mut st);
        });
    }

    fn reset(&self, st: &mut State) {
        st.running = false;
        st.out = None;
        st.feed_path = None;
        if let Some(waiter) = st.feed_waiter.take() {
            let _ = waiter.send(String::new());
        }
        if let Some(task) = st.accept_task.take() {
            task.abort();
        }
    }

    async fn accept(self, listener: UnixListener) {
        while let Ok((stream, _)) = listener.accept().await {
            let conn = self.inner.next_conn.fetch_add(1, Ordering::Relaxed);
            let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
            let (mut rd, mut wr) = stream.into_split();
            {
                let mut st = self.state();
                if st.tap {
                    let _ = tx.send(frame(OP_VISUALIZER, 0, &[1]));
                }
                for queued in st.queue.drain(..) {
                    let _ = tx.send(queued);
                }
                st.out = Some((conn, tx));
            }
            tokio::spawn(async move {
                while let Some(buf) = rx.recv().await {
                    if wr.write_all(&buf).await.is_err() {
                        break;
                    }
                }
            });
            let host = self.clone();
            tokio::spawn(async move {
                let mut framer = Framer::new();
                let mut buf = vec![0u8; 64 * 1024];
                while let Ok(n) = rd.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    framer.push(&buf[..n]);
                    while let Some(msg) = framer.next_message() {
                        host.on_reply(msg.op, msg.id, &msg.rest);
                    }
                }
                let mut st = host.state();
                if st.out.as_ref().is_some_and(|(c, _)| *c == conn) {
                    st.out = None;
                }
            });
        }
    }

    fn send(&self, buf: Vec<u8>) {
        self.start();
        let mut st = self.state();
        if st.unavailable {
            return;
        }
        match &st.out {
            Some((_, tx)) => {
                let _ = tx.send(buf);
            }
            None => st.queue.push(buf),
        }
    }

    fn on_reply(&self, op: u8, id: u32, rest: &[u8]) {
        let u16_at =
            |at: usize| rest.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        let u32_at = |at: usize| {
            rest.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        match op {
            REPLY_PORT => {
                if let Some(w) = self.state().port_waiters.remove(&id) {
                    let _ = w.send(u16_at(0));
                }
            }
            REPLY_CONFIG => {
                let codec = if rest.first() == Some(&1) { Codec::H265 } else { Codec::H264 };
                let atom = rest.get(1..).unwrap_or_default().to_vec();
                let _ = self.inner.events.send(HostEvent::Config { plane: id, codec, atom });
            }
            REPLY_STARTED => {
                let _ = self.inner.events.send(HostEvent::Started { plane: id });
            }
            REPLY_AUDIO_PORTS => {
                if let Some(w) = self.state().audio_waiters.remove(&id) {
                    let ports = if rest.len() >= 4 { (u16_at(0), u16_at(2)) } else { (0, 0) };
                    let _ = w.send(ports);
                }
            }
            REPLY_AUDIO_STARTED => {
                let _ = self
                    .inner
                    .events
                    .send(HostEvent::AudioStarted { stream: id, first_sample: u32_at(0) });
            }
            REPLY_VISUALIZER => {
                let bands =
                    rest.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
                let _ = self.inner.events.send(HostEvent::Spectrum { bands });
            }
            REPLY_FEED => {
                if let Some(w) = self.state().feed_waiter.take() {
                    let _ = w.send(String::from_utf8_lossy(rest).into_owned());
                }
            }
            REPLY_URL_STATUS => {
                if let Some(status) = livi_host_proto::parse_url_status(rest) {
                    let _ = self.inner.events.send(HostEvent::UrlStatus { plane: id, status });
                }
            }
            _ => {}
        }
    }

    pub async fn open_feed(&self) -> String {
        let _one_at_a_time = self.inner.feed_lock.lock().await;
        if let Some(path) = self.state().feed_path.clone() {
            return path;
        }
        let path = self.inner.feed_path.display().to_string();
        let (tx, rx) = oneshot::channel();
        self.state().feed_waiter = Some(tx);
        self.send(frame(OP_FEED_OPEN, 0, path.as_bytes()));
        let bound = tokio::time::timeout(ANSWER_TIMEOUT, rx)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let mut st = self.state();
        st.feed_waiter = None;
        if !bound.is_empty() {
            st.feed_path = Some(bound.clone());
        }
        bound
    }

    pub async fn open_video_receiver(
        &self,
        plane: u32,
        key: &[u8; 32],
        cluster: bool,
    ) -> (u16, u32) {
        let receiver = self.next_id();
        let (tx, rx) = oneshot::channel();
        self.state().port_waiters.insert(receiver, tx);
        let mut rest = plane.to_le_bytes().to_vec();
        rest.push(u8::from(cluster));
        rest.extend_from_slice(key);
        self.send(frame(OP_LISTEN, receiver, &rest));
        let port = tokio::time::timeout(ANSWER_TIMEOUT, rx).await.ok().and_then(Result::ok);
        if port.is_none() {
            self.state().port_waiters.remove(&receiver);
        }
        (port.unwrap_or(0), receiver)
    }

    pub fn set_active_feeder(&self, receiver: u32, active: bool) {
        self.send(frame(OP_SET_ACTIVE, receiver, &[u8::from(active)]));
    }

    pub fn close_video_receiver(&self, receiver: u32) {
        self.send(frame(OP_TEARDOWN, receiver, &[]));
    }

    /// A fed stream of its own into a plane or an audio stream, switched like a receiver: the
    /// active one plays and holds the others of the same target.
    pub fn open_feed_route(&self, target: FeedTarget, active: bool) -> u32 {
        let id = self.next_id();
        let (kind, target) = match target {
            FeedTarget::Plane(plane) => (0u8, plane),
            FeedTarget::Audio(stream) => (1, stream),
        };
        let mut rest = vec![kind];
        rest.extend_from_slice(&target.to_le_bytes());
        rest.push(u8::from(active));
        self.send(frame(OP_FEED_ROUTE, id, &rest));
        id
    }

    pub fn close_feed_route(&self, id: u32) {
        self.send(frame(OP_FEED_CLOSE, id, &[]));
    }

    /// (stream id, data port, control port), ports 0 on a timeout.
    pub async fn open_audio(&self, key: &[u8; 32], o: &AudioOpts) -> (u32, u16, u16) {
        let stream = self.next_id();
        let (tx, rx) = oneshot::channel();
        self.state().audio_waiters.insert(stream, tx);
        let mut rest = vec![o.codec as u8, o.payload_type];
        rest.extend_from_slice(&o.clock_rate.to_le_bytes());
        rest.push(o.channels);
        rest.extend_from_slice(&o.latency_ms.to_le_bytes());
        rest.push(u8::from(o.realtime) | (u8::from(o.fed) << 1));
        rest.extend_from_slice(key);
        rest.extend_from_slice(o.device.as_bytes());
        self.send(frame(OP_AUDIO_OPEN, stream, &rest));
        let ports = tokio::time::timeout(ANSWER_TIMEOUT, rx).await.ok().and_then(Result::ok);
        if ports.is_none() {
            self.state().audio_waiters.remove(&stream);
        }
        let (data, control) = ports.unwrap_or((0, 0));
        (stream, data, control)
    }

    pub fn push_audio(&self, stream: u32, samples: &[u8]) {
        self.send(frame(OP_AUDIO_DATA, stream, samples));
    }

    pub fn set_visualizer_tap(&self, on: bool) {
        let connected = {
            let mut st = self.state();
            st.tap = on;
            st.out.is_some()
        };
        if connected {
            self.send(frame(OP_VISUALIZER, 0, &[u8::from(on)]));
        } else if on {
            self.start();
        }
    }

    /// `ramp_ms` 0 sets it at once, a ramp runs on the pipeline clock.
    pub fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32) {
        let mut rest = level.to_le_bytes().to_vec();
        rest.extend_from_slice(&ramp_ms.to_le_bytes());
        self.send(frame(OP_AUDIO_VOLUME, stream, &rest));
    }

    /// Moves every open stream, empty for the system's default.
    pub fn set_audio_output(&self, device: &str) {
        if self.state().running {
            self.send(frame(OP_AUDIO_OUTPUT, 0, device.as_bytes()));
        }
    }

    /// Only the active phone's streams reach the sink.
    pub fn set_audio_active(&self, stream: u32, active: bool) {
        self.send(frame(OP_AUDIO_ACTIVE, stream, &[u8::from(active)]));
    }

    pub fn close_audio(&self, stream: u32) {
        self.state().audio_waiters.remove(&stream);
        self.send(frame(OP_AUDIO_STOP, stream, &[]));
    }

    pub fn open_mic(&self, key: &[u8; 32], o: &MicOpts) -> u32 {
        let id = self.next_id();
        let mut rest = vec![u8::from(o.pcm), o.payload_type];
        rest.extend_from_slice(&o.sample_rate.to_le_bytes());
        rest.push(o.channels);
        rest.extend_from_slice(&o.bitrate.to_le_bytes());
        rest.extend_from_slice(&o.frame_ms.to_le_bytes());
        rest.extend_from_slice(&o.port.to_le_bytes());
        rest.extend_from_slice(key);
        let phone = o.phone.as_bytes();
        rest.push(phone.len() as u8);
        rest.extend_from_slice(phone);
        rest.extend_from_slice(o.device.as_bytes());
        self.send(frame(OP_MIC_OPEN, id, &rest));
        id
    }

    pub fn close_mic(&self, id: u32) {
        self.send(frame(OP_MIC_STOP, id, &[]));
    }

    pub fn open_mic_tap(&self, path: &str, sample_rate: u32, channels: u8, device: &str) -> u32 {
        let id = self.next_id();
        let mut rest = sample_rate.to_le_bytes().to_vec();
        rest.push(channels);
        rest.push(device.len() as u8);
        rest.extend_from_slice(device.as_bytes());
        rest.extend_from_slice(path.as_bytes());
        self.send(frame(OP_TAP_OPEN, id, &rest));
        id
    }

    pub fn close_mic_tap(&self, id: u32) {
        self.send(frame(OP_TAP_STOP, id, &[]));
    }

    pub fn create_player(&self, plane: u32, codec: Codec, codec_data: &[u8]) {
        self.send(frame(
            OP_CREATE,
            plane,
            &livi_host_proto::plane_body(codec.as_str(), codec_data),
        ));
    }

    /// The host fetches and times the stream itself, an empty device is the system default.
    pub fn play_url(&self, plane: u32, url: &str, audio_device: &str) {
        self.send(frame(OP_PLAY_URL, plane, &livi_host_proto::url_body(url, audio_device)));
    }

    /// 0 pauses.
    pub fn set_play_rate(&self, plane: u32, rate: f64) {
        self.send(frame(OP_PLAY_RATE, plane, &rate.to_le_bytes()));
    }

    pub fn seek(&self, plane: u32, seconds: f64) {
        self.send(frame(OP_PLAY_SEEK, plane, &seconds.to_le_bytes()));
    }

    pub fn set_play_muted(&self, plane: u32, muted: bool) {
        self.send(frame(OP_PLAY_MUTE, plane, &[u8::from(muted)]));
    }

    pub fn stop(&self, plane: u32) {
        self.send(frame(OP_STOP, plane, &[]));
    }
}

fn log_file(path: &std::path::Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::File::create(path).ok()
}

pub async fn probe_codecs(launch: &Launch) -> Option<serde_json::Value> {
    let run = tokio::process::Command::new(&launch.bin)
        .arg("--probe")
        .envs(launch.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match tokio::time::timeout(Duration::from_secs(10), run).await {
        Ok(Ok(out)) => {
            serde_json::from_slice(String::from_utf8_lossy(&out.stdout).trim().as_bytes()).ok()
        }
        _ => {
            eprintln!("[core] gst-host codec probe failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use livi_host_proto::encode_reply;
    use tokio::net::UnixStream;

    use super::*;
    use crate::test_dir::TempDir;

    struct FakeHost {
        stream: UnixStream,
        framer: Framer,
    }

    impl FakeHost {
        async fn connect(host: &GstHost) -> Self {
            host.start();
            let stream = UnixStream::connect(&host.inner.sock_path).await.unwrap();
            Self { stream, framer: Framer::new() }
        }

        async fn next(&mut self) -> livi_host_proto::Message {
            let mut buf = [0u8; 4096];
            loop {
                if let Some(m) = self.framer.next_message() {
                    return m;
                }
                let n = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buf))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(n > 0, "core closed the socket");
                self.framer.push(&buf[..n]);
            }
        }

        async fn reply(&mut self, op: u8, id: u32, rest: &[u8]) {
            self.stream.write_all(&encode_reply(op, id, rest)).await.unwrap();
        }
    }

    fn host(dir: &TempDir) -> GstHost {
        GstHost::new(dir.0.join("gst.sock"))
    }

    #[tokio::test]
    async fn commands_wait_for_the_host_then_arrive_in_order() {
        let dir = TempDir::new();
        let gst = host(&dir);
        gst.set_visualizer_tap(true);
        gst.stop(0x7a00_0001);
        let mut fake = FakeHost::connect(&gst).await;
        let first = fake.next().await;
        assert_eq!((first.op, first.rest.as_slice()), (OP_VISUALIZER, &[1u8][..]));
        assert_eq!(fake.next().await.op, OP_STOP);
        gst.create_player(0x7a00_0001, Codec::H265, &[9, 9]);
        let create = fake.next().await;
        assert_eq!((create.op, create.id), (OP_CREATE, 0x7a00_0001));
        assert_eq!(create.rest, [&[4u8][..], b"h265", &[9, 9]].concat());
    }

    #[tokio::test]
    async fn the_tap_follows_the_host_through_a_restart_and_brings_the_bands() {
        let dir = TempDir::new();
        let gst = host(&dir);
        let mut events = gst.subscribe();
        gst.set_visualizer_tap(true);
        let mut first = FakeHost::connect(&gst).await;
        assert_eq!(first.next().await.rest, [1]);
        let bands: Vec<u8> = [0.25f32, 1.0].iter().flat_map(|b| b.to_le_bytes()).collect();
        first.reply(REPLY_VISUALIZER, 0, &bands).await;
        assert_eq!(events.recv().await.unwrap(), HostEvent::Spectrum { bands: vec![0.25, 1.0] });
        let status = livi_host_proto::UrlStatus { position: Some(2.0), ..Default::default() };
        first
            .reply(REPLY_URL_STATUS, 0x7a00_0002, &livi_host_proto::url_status_body(&status))
            .await;
        first.reply(REPLY_URL_STATUS, 0x7a00_0002, &[1]).await;
        assert_eq!(
            events.recv().await.unwrap(),
            HostEvent::UrlStatus { plane: 0x7a00_0002, status }
        );
        drop(first);

        let mut again = FakeHost::connect(&gst).await;
        assert_eq!((again.next().await.op, gst.state().tap), (OP_VISUALIZER, true));
        gst.set_visualizer_tap(false);
        assert_eq!(again.next().await.rest, [0]);
    }

    #[tokio::test]
    async fn a_new_output_reaches_a_running_host_and_starts_none() {
        let dir = TempDir::new();
        let gst = host(&dir);
        gst.set_audio_output("headphones");
        assert!(!gst.state().running);
        let mut fake = FakeHost::connect(&gst).await;
        gst.set_audio_output("headphones");
        let output = fake.next().await;
        assert_eq!((output.op, output.rest.as_slice()), (OP_AUDIO_OUTPUT, &b"headphones"[..]));
    }

    #[tokio::test]
    async fn a_tap_switched_off_before_any_host_sends_nothing() {
        let dir = TempDir::new();
        let gst = host(&dir);
        gst.set_visualizer_tap(false);
        gst.stop(0x7a00_0001);
        let mut fake = FakeHost::connect(&gst).await;
        assert_eq!(fake.next().await.op, OP_STOP);
    }

    #[tokio::test]
    async fn a_feed_route_names_its_target_and_closes_again() {
        let dir = TempDir::new();
        let gst = host(&dir);
        let mut fake = FakeHost::connect(&gst).await;
        let video = gst.open_feed_route(FeedTarget::Plane(0x7a00_0001), true);
        let audio = gst.open_feed_route(FeedTarget::Audio(0x7b00_0009), false);
        assert_ne!(video, audio);
        let first = fake.next().await;
        assert_eq!((first.op, first.id), (OP_FEED_ROUTE, video));
        assert_eq!(first.rest, [0, 1, 0, 0, 0x7a, 1]);
        let second = fake.next().await;
        assert_eq!((second.id, second.rest.as_slice()), (audio, &[1, 9, 0, 0, 0x7b, 0][..]));
        gst.close_feed_route(video);
        let close = fake.next().await;
        assert_eq!((close.op, close.id), (OP_FEED_CLOSE, video));
    }

    #[tokio::test]
    async fn a_screen_receiver_gets_its_port() {
        let dir = TempDir::new();
        let gst = host(&dir);
        let mut fake = FakeHost::connect(&gst).await;
        let open = tokio::spawn({
            let gst = gst.clone();
            async move { gst.open_video_receiver(0x7a00_0001, &[7; 32], false).await }
        });
        let listen = fake.next().await;
        assert_eq!(listen.op, OP_LISTEN);
        assert_eq!(&listen.rest[..5], &[1, 0, 0, 0x7a, 0]);
        assert_eq!(&listen.rest[5..], &[7; 32]);
        fake.reply(REPLY_PORT, listen.id, &7100u16.to_le_bytes()).await;
        assert_eq!(open.await.unwrap(), (7100, listen.id));
    }

    #[tokio::test]
    async fn audio_ports_and_events_come_back() {
        let dir = TempDir::new();
        let gst = host(&dir);
        let mut events = gst.subscribe();
        let mut fake = FakeHost::connect(&gst).await;
        let opts = AudioOpts {
            codec: AudioCodec::Opus,
            payload_type: 100,
            clock_rate: 48000,
            channels: 1,
            latency_ms: 1000,
            realtime: true,
            fed: false,
            device: "sink".into(),
        };
        let open = tokio::spawn({
            let gst = gst.clone();
            async move { gst.open_audio(&[1; 32], &opts).await }
        });
        let req = fake.next().await;
        assert_eq!(&req.rest[..12], &[1, 100, 0x80, 0xbb, 0, 0, 1, 0xe8, 3, 0, 0, 1]);
        assert_eq!(&req.rest[44..], b"sink");
        fake.reply(REPLY_AUDIO_PORTS, req.id, &[0x10, 0x27, 0x11, 0x27]).await;
        assert_eq!(open.await.unwrap(), (req.id, 10000, 10001));

        fake.reply(REPLY_CONFIG, 0x7a00_0001, &[1, 0xaa, 0xbb]).await;
        fake.reply(REPLY_AUDIO_STARTED, req.id, &5u32.to_le_bytes()).await;
        assert_eq!(
            events.recv().await.unwrap(),
            HostEvent::Config { plane: 0x7a00_0001, codec: Codec::H265, atom: vec![0xaa, 0xbb] }
        );
        assert_eq!(
            events.recv().await.unwrap(),
            HostEvent::AudioStarted { stream: req.id, first_sample: 5 }
        );
    }

    #[tokio::test]
    async fn an_unanswered_request_times_out_with_zeros() {
        let dir = TempDir::new();
        let gst = host(&dir);
        tokio::time::pause();
        let open = tokio::spawn({
            let gst = gst.clone();
            async move { gst.open_video_receiver(1, &[0; 32], true).await }
        });
        tokio::time::advance(ANSWER_TIMEOUT + Duration::from_millis(10)).await;
        assert_eq!(open.await.unwrap().0, 0);
        assert!(gst.state().port_waiters.is_empty());
    }

    #[tokio::test]
    async fn the_feed_is_opened_once() {
        let dir = TempDir::new();
        let gst = host(&dir);
        let mut fake = FakeHost::connect(&gst).await;
        let open = tokio::spawn({
            let gst = gst.clone();
            async move { gst.open_feed().await }
        });
        let req = fake.next().await;
        assert_eq!(req.op, OP_FEED_OPEN);
        fake.reply(REPLY_FEED, 0, &req.rest).await;
        let path = open.await.unwrap();
        assert!(path.ends_with("gst.sock.feed"));
        assert_eq!(gst.open_feed().await, path);
    }

    #[tokio::test]
    async fn a_missing_binary_makes_the_host_unavailable() {
        let dir = TempDir::new();
        let gst = host(&dir);
        gst.set_launch(Launch {
            bin: dir.0.join("no-such-host"),
            env: Vec::new(),
            log: dir.0.join("log/gst-host.log"),
            crash: dir.0.join("crash.log"),
        });
        assert_eq!(gst.open_feed().await, "");
        assert!(gst.state().unavailable);
    }
}
