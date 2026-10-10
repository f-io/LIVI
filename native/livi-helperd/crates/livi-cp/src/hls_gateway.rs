//! Stands between the player on this machine and the video app's HLS server on the phone. Every
//! request shares one connection upstream, playlists lose their low-latency lines, and a stream
//! that starts over is reported, so the player can be opened afresh in the new size.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio::task::{JoinHandle, JoinSet};

use crate::hls::{self, Client, Playlist};

/// hlsdemux2 aborts the whole process when one blocking playlist reload overlaps the next, so
/// it only ever sees whole segments.
const LOW_LATENCY: [&str; 5] = [
    "#EXT-X-SERVER-CONTROL",
    "#EXT-X-PART",
    "#EXT-X-PRELOAD-HINT",
    "#EXT-X-RENDITION-REPORT",
    "#EXT-X-SKIP",
];
const MAX_HEAD: usize = 16 * 1024;
/// The first playlist of a fresh tunnel sometimes fails, a player gives up on it at once.
const PLAYLIST_ATTEMPTS: usize = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(250);

pub struct Gateway {
    url: String,
    task: JoinHandle<()>,
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Gateway {
    /// `playlist` is the stream's URL behind the tunnel. Each start over sends on `started_over`.
    pub async fn open(playlist: &str, started_over: mpsc::UnboundedSender<()>) -> io::Result<Self> {
        let (host, path) = hls::split_url(playlist)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://{}{path}", listener.local_addr()?);
        let upstream = Arc::new(Upstream {
            origin: format!("http://{host}"),
            client: Mutex::new(Client::default()),
            seen: std::sync::Mutex::new(HashMap::new()),
            started_over,
        });
        let task = tokio::spawn(async move {
            // Dropping the set with the aborted task ends every open connection too.
            let mut served = JoinSet::new();
            while let Ok((sock, _)) = listener.accept().await {
                served.spawn(serve(upstream.clone(), sock));
                while served.try_join_next().is_some() {}
            }
        });
        Ok(Self { url, task })
    }

    /// Where the player fetches the stream.
    pub fn url(&self) -> &str {
        &self.url
    }
}

struct Upstream {
    origin: String,
    client: Mutex<Client>,
    /// What each media playlist looked like last, by path.
    seen: std::sync::Mutex<HashMap<String, Mark>>,
    started_over: mpsc::UnboundedSender<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mark {
    init: Option<String>,
    discontinuity: u64,
    newest: u64,
}

impl Mark {
    fn of(list: &Playlist) -> Self {
        Self {
            init: list.init.clone(),
            discontinuity: list.discontinuity,
            newest: list.first + list.segments.len() as u64,
        }
    }

    /// The same stream further on: same init segment and discontinuity, numbering not gone back.
    fn continued_by(&self, next: &Mark) -> bool {
        next.init == self.init
            && next.discontinuity == self.discontinuity
            && next.newest >= self.newest
    }
}

impl Upstream {
    async fn fetch(&self, path: &str) -> io::Result<Vec<u8>> {
        let url = format!("{}{path}", self.origin);
        let attempts = if is_playlist(path) { PLAYLIST_ATTEMPTS } else { 1 };
        let mut client = self.client.lock().await;
        let mut failed = 0;
        loop {
            match client.get(&url).await {
                Err(e) if failed + 1 < attempts => {
                    failed += 1;
                    println!("[hlsGateway] {path}: {e}, trying again");
                    tokio::time::sleep(RETRY_PAUSE).await;
                }
                other => return other,
            }
        }
    }

    /// Playlists go out without their low-latency lines, everything else as it came.
    fn answer(&self, path: &str, body: Vec<u8>) -> Vec<u8> {
        if !body.starts_with(b"#EXTM3U") {
            return body;
        }
        let text = String::from_utf8_lossy(&body);
        let list = hls::read_playlist(&text);
        if !list.segments.is_empty() {
            self.note(path, Mark::of(&list));
        }
        whole_segments(&text).into_bytes()
    }

    fn note(&self, path: &str, now: Mark) {
        let key = path.split('?').next().unwrap_or(path).to_string();
        let before = self.seen.lock().unwrap_or_else(|e| e.into_inner()).insert(key, now.clone());
        if before.is_some_and(|before| !before.continued_by(&now)) {
            let _ = self.started_over.send(());
        }
    }
}

fn is_playlist(path: &str) -> bool {
    path.split('?').next().unwrap_or(path).ends_with(".m3u8")
}

fn whole_segments(text: &str) -> String {
    text.lines()
        .filter(|line| !LOW_LATENCY.iter().any(|tag| line.trim_start().starts_with(tag)))
        .flat_map(|line| [line, "\n"])
        .collect()
}

async fn serve(upstream: Arc<Upstream>, mut sock: TcpStream) {
    let mut buf = Vec::new();
    while let Some(ask) = read_request(&mut sock, &mut buf).await {
        let (status, body) = match upstream.fetch(&ask.path).await {
            Ok(body) => ("200 OK", upstream.answer(&ask.path, body)),
            Err(e) => {
                println!("[hlsGateway] {}: {e}", ask.path);
                ("502 Bad Gateway", Vec::new())
            }
        };
        let kind = if is_playlist(&ask.path) {
            "application/vnd.apple.mpegurl"
        } else {
            "application/octet-stream"
        };
        let close = if ask.close { "Connection: close\r\n" } else { "" };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{close}\r\n",
            body.len()
        );
        if sock.write_all(head.as_bytes()).await.is_err()
            || (!ask.head_only && sock.write_all(&body).await.is_err())
            || ask.close
        {
            return;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Request {
    path: String,
    head_only: bool,
    /// The client closes the connection after the answer.
    close: bool,
}

async fn read_request(sock: &mut TcpStream, buf: &mut Vec<u8>) -> Option<Request> {
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            buf.drain(..end + 4);
            let mut lines = head.lines();
            let mut words = lines.next()?.split(' ');
            let (method, path) = (words.next()?, words.next()?);
            let close = lines
                .map(str::to_ascii_lowercase)
                .any(|l| l.starts_with("connection:") && l.contains("close"));
            return Some(Request { path: path.to_string(), head_only: method == "HEAD", close });
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
        let mut chunk = [0u8; 4096];
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    const LIVE: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n\
        #EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=0.6\n\
        #EXT-X-PART-INF:PART-TARGET=0.2\n#EXT-X-MEDIA-SEQUENCE:5\n\
        #EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\nseg_5.m4s\n\
        #EXT-X-PART:DURATION=0.2,URI=\"part_6_0.m4s\"\n\
        #EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"part_6_1.m4s\"\n\
        #EXT-X-RENDITION-REPORT:URI=\"other.m3u8\",LAST-MSN=5\n";

    #[test]
    fn a_playlist_keeps_only_whole_segments() {
        assert_eq!(
            whole_segments(LIVE),
            "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:5\n\
             #EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\nseg_5.m4s\n"
        );
        assert_eq!(whole_segments("#EXT-X-SKIP:SKIPPED-SEGMENTS=3\r\nseg.m4s\r\n"), "seg.m4s\n");
    }

    #[test]
    fn only_a_new_init_discontinuity_or_numbering_going_back_starts_over() {
        let mark = |init: &str, discontinuity, newest| Mark {
            init: Some(init.into()),
            discontinuity,
            newest,
        };
        let before = mark("init.mp4", 0, 10);
        assert!(before.continued_by(&mark("init.mp4", 0, 10)));
        assert!(before.continued_by(&mark("init.mp4", 0, 12)));
        assert!(!before.continued_by(&mark("init_1.mp4", 0, 12)));
        assert!(!before.continued_by(&mark("init.mp4", 1, 12)));
        assert!(!before.continued_by(&mark("init.mp4", 0, 3)));
        assert!(is_playlist("/a/index.m3u8?_HLS_msn=3") && !is_playlist("/a/seg.m4s"));
    }

    struct Phone {
        base: String,
        playlist: Arc<StdMutex<String>>,
        connections: Arc<StdMutex<usize>>,
        /// Requests left that the server answers with nothing at all.
        dropped: Arc<StdMutex<usize>>,
    }

    /// The video app's server: one live playlist, its segments, keep-alive connections.
    async fn phone() -> Phone {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let playlist = Arc::new(StdMutex::new(LIVE.to_string()));
        let connections = Arc::new(StdMutex::new(0));
        let dropped = Arc::new(StdMutex::new(0));
        let (list, count, drop) = (playlist.clone(), connections.clone(), dropped.clone());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                *count.lock().unwrap() += 1;
                let (list, drop) = (list.clone(), drop.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    while let Some(ask) = read_request(&mut sock, &mut buf).await {
                        if std::mem::replace(&mut *drop.lock().unwrap(), 0) > 0 {
                            return;
                        }
                        let body = match ask.path.as_str() {
                            "/live/index.m3u8" => list.lock().unwrap().clone().into_bytes(),
                            "/live/seg_5.m4s" => b"segment five".to_vec(),
                            _ => {
                                let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await;
                                return;
                            }
                        };
                        let head =
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                        if sock.write_all(head.as_bytes()).await.is_err()
                            || sock.write_all(&body).await.is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });
        Phone { base, playlist, connections, dropped }
    }

    /// One GET with `extra` header lines, the whole answer as text.
    async fn ask(url: &str, extra: &str) -> String {
        let (host, path) = hls::split_url(url).unwrap();
        let mut sock = TcpStream::connect(host).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n");
        sock.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut chunk))
                .await
                .unwrap()
                .unwrap();
            answer.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&answer).to_string();
            let whole = text.split_once("\r\n\r\n").is_some_and(|(head, body)| {
                let len = head.lines().find_map(|l| l.strip_prefix("Content-Length: "));
                len.and_then(|v| v.parse::<usize>().ok()).is_some_and(|len| body.len() >= len)
            });
            if whole || n == 0 {
                return text;
            }
        }
    }

    #[tokio::test]
    async fn the_player_gets_whole_segments_over_one_connection_to_the_phone() {
        let phone = phone().await;
        let (tx, _rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(&format!("{}/live/index.m3u8", phone.base), tx).await.unwrap();
        assert!(gateway.url().starts_with("http://127.0.0.1:"));
        assert!(gateway.url().ends_with("/live/index.m3u8"));

        let list = ask(gateway.url(), "").await;
        assert!(list.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(list.contains("Content-Type: application/vnd.apple.mpegurl"));
        assert!(list.ends_with("seg_5.m4s\n") && !list.contains("EXT-X-PART"));

        let segment = gateway.url().replace("index.m3u8", "seg_5.m4s");
        let asks: Vec<_> = (0..3).map(|_| tokio::spawn(ask_owned(segment.clone()))).collect();
        for answer in asks {
            assert!(answer.await.unwrap().ends_with("\r\n\r\nsegment five"));
        }
        assert_eq!(*phone.connections.lock().unwrap(), 1);

        let missing = ask(&gateway.url().replace("index.m3u8", "gone.m4s"), "").await;
        assert!(missing.starts_with("HTTP/1.1 502 Bad Gateway\r\n"));
    }

    async fn ask_owned(url: String) -> String {
        ask(&url, "").await
    }

    #[tokio::test]
    async fn a_stream_that_starts_over_is_reported_once() {
        let phone = phone().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(&format!("{}/live/index.m3u8", phone.base), tx).await.unwrap();
        ask(gateway.url(), "").await;
        ask(gateway.url(), "").await;
        assert!(rx.try_recv().is_err());

        let turned = LIVE.replace("init.mp4", "init_1.mp4");
        *phone.playlist.lock().unwrap() = turned;
        ask(gateway.url(), "").await;
        ask(gateway.url(), "").await;
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "the new stream is the one compared against now");
    }

    #[tokio::test]
    async fn a_failed_playlist_is_asked_again_and_head_and_close_are_honoured() {
        let phone = phone().await;
        let (tx, _rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(&format!("{}/live/index.m3u8", phone.base), tx).await.unwrap();
        *phone.dropped.lock().unwrap() = 1;
        assert!(ask(gateway.url(), "").await.starts_with("HTTP/1.1 200 OK\r\n"));

        let head = ask(gateway.url(), "Connection: close\r\n").await;
        assert!(head.contains("Connection: close\r\n"));
        let (host, path) = hls::split_url(gateway.url()).unwrap();
        let mut sock = TcpStream::connect(host).await.unwrap();
        sock.write_all(format!("HEAD {path} HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut answer = vec![0u8; 4096];
        let n = sock.read(&mut answer).await.unwrap();
        let answer = String::from_utf8_lossy(&answer[..n]).to_string();
        assert!(answer.starts_with("HTTP/1.1 200 OK") && answer.ends_with("\r\n\r\n"));

        let mut endless = TcpStream::connect(host).await.unwrap();
        let _ = endless.write_all(&vec![b'x'; MAX_HEAD + 4096]).await;
        let read = endless.read(&mut [0u8; 16]).await;
        assert!(matches!(read, Ok(0) | Err(_)), "an endless head gets no answer");
        assert!(Gateway::open("https://phone/a.m3u8", mpsc::unbounded_channel().0).await.is_err());
    }
}
