// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio::task::{JoinHandle, JoinSet};

use crate::hls::{self, Client, Conn, Dialer, FETCH_TIMEOUT, Playlist, Target};

const BLOCKING: [&str; 3] = ["CAN-BLOCK-RELOAD", "CAN-SKIP-UNTIL", "CAN-SKIP-DATERANGES"];
const MAX_HEAD: usize = 16 * 1024;
const PLAYLIST_ATTEMPTS: usize = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(250);
const ELSEWHERE: &str = "/~/";

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
    pub async fn open(
        client: Client,
        playlist: &str,
        started_over: mpsc::UnboundedSender<()>,
    ) -> io::Result<Self> {
        let (origin, path) = match Target::of(playlist) {
            Ok(target) => (target.origin(), target.path.to_string()),
            Err(_) => (String::new(), elsewhere(playlist)),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://{}{path}", listener.local_addr()?);
        let upstream = Arc::new(Upstream {
            origin,
            client: Mutex::new(client),
            seen: std::sync::Mutex::new(HashMap::new()),
            started_over,
            next_parts: std::sync::Mutex::default(),
        });
        let task = tokio::spawn(async move {
            let mut served = JoinSet::new();
            while let Ok((sock, _)) = listener.accept().await {
                served.spawn(serve(upstream.clone(), sock));
                while served.try_join_next().is_some() {}
            }
        });
        Ok(Self { url, task })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

struct Upstream {
    origin: String,
    client: Mutex<Client>,
    seen: std::sync::Mutex<HashMap<String, Mark>>,
    started_over: mpsc::UnboundedSender<()>,
    next_parts: std::sync::Mutex<HashMap<String, (u64, u64)>>,
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

    fn continued_by(&self, next: &Mark) -> bool {
        next.init == self.init
            && next.discontinuity == self.discontinuity
            && next.newest >= self.newest
    }
}

fn elsewhere(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => format!("{ELSEWHERE}{scheme}/{rest}"),
        None => format!("{ELSEWHERE}{url}"),
    }
}

impl Upstream {
    fn source(&self, path: &str) -> String {
        match path
            .strip_prefix(ELSEWHERE)
            .and_then(|rest| rest.split_once('/'))
        {
            Some((scheme, rest)) => format!("{scheme}://{rest}"),
            None => format!("{}{path}", self.origin),
        }
    }

    fn local(&self, source: &str, uri: &str) -> Option<String> {
        let absolute = uri.starts_with("http://") || uri.starts_with("https://");
        if !absolute && !uri.starts_with('/') {
            return None;
        }
        let url = hls::resolve(source, uri);
        match url.strip_prefix(self.origin.as_str()) {
            Some(path) if !self.origin.is_empty() && path.starts_with('/') => Some(path.into()),
            _ => Some(elsewhere(&url)),
        }
    }

    async fn fetch_playlist(&self, path: &str) -> io::Result<Vec<u8>> {
        let url = self.source(path);
        let key = path.split('?').next().unwrap_or(path);
        let waits_for = self
            .next_parts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .copied();
        let blocking = waits_for
            .filter(|_| !path.contains("_HLS_"))
            .map(|(msn, part)| hls::with_query(&url, &format!("_HLS_msn={msn}&_HLS_part={part}")));
        let mut client = self.client.lock().await;
        let mut failed = 0;
        loop {
            let ask = blocking.as_deref().filter(|_| failed == 0).unwrap_or(&url);
            match client.get(ask).await {
                Err(e) if failed + 1 < PLAYLIST_ATTEMPTS => {
                    failed += 1;
                    println!("[hlsGateway] {path}: {e}, trying again");
                    tokio::time::sleep(RETRY_PAUSE).await;
                }
                other => return other,
            }
        }
    }

    async fn pass(&self, ask: &Request, player: &mut TcpStream) -> io::Result<Passed> {
        let url = self.source(&ask.path);
        let target = Target::of(&url)?;
        let origin = target.origin();
        let (kept, dialer) = {
            let mut client = self.client.lock().await;
            (client.take(&origin), client.dialer())
        };
        let method = if ask.head_only { "HEAD" } else { "GET" };
        let request = format!(
            "{method} {} HTTP/1.1\r\nHost: {}\r\n{}\r\n",
            target.path, target.host, ask.range
        );
        let reused = kept.is_some();
        let (mut conn, head, early) = match ask_source(&dialer, &target, kept, &request).await {
            Err(e) if reused && e.kind() != io::ErrorKind::TimedOut => {
                ask_source(&dialer, &target, None, &request).await
            }
            other => other,
        }
        .map_err(|e| io::Error::new(e.kind(), format!("{url}: {e}")))?;
        let lower = head.to_ascii_lowercase();
        let body = Body::of(&lower, ask.head_only);
        let reusable =
            !matches!(body, Body::ToClose) && header(&lower, "connection") != Some("close");
        let found_ok = lower.split(' ').nth(1) == Some("200");
        let playlist = header(&lower, "content-type").is_some_and(|t| t.contains("mpegurl"));
        if found_ok && playlist {
            let mut list = Vec::new();
            return match body.copy(&early, &mut conn, &mut list).await {
                Ok(()) => {
                    if reusable {
                        self.client.lock().await.keep(origin, conn);
                    }
                    Ok(Passed::Playlist(list))
                }
                Err(cut) => {
                    let e = cut.error();
                    Err(io::Error::new(e.kind(), format!("{url}: {e}")))
                }
            };
        }
        let close = ask.close || matches!(body, Body::ToClose);
        if player
            .write_all(self.head_for_player(&head, &url, close).as_bytes())
            .await
            .is_err()
        {
            return Ok(Passed::Through(false));
        }
        match body.copy(&early, &mut conn, player).await {
            Ok(()) => {
                if reusable {
                    self.client.lock().await.keep(origin, conn);
                }
                Ok(Passed::Through(!close))
            }
            Err(Cut::Source(e)) => {
                println!("[hlsGateway] {url}: cut off: {e}");
                Ok(Passed::Through(false))
            }
            Err(Cut::Player) => Ok(Passed::Through(false)),
        }
    }

    fn head_for_player(&self, head: &str, url: &str, close: bool) -> String {
        let mut lines = head.lines().map(|l| l.trim_end_matches('\r'));
        let mut out = format!("{}\r\n", lines.next().unwrap_or_default());
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            match name.trim().to_ascii_lowercase().as_str() {
                "connection" | "keep-alive" | "proxy-connection" => continue,
                "location" => match self.local(url, value.trim()) {
                    Some(local) => out.push_str(&format!("{name}: {local}\r\n")),
                    None => out.push_str(&format!("{line}\r\n")),
                },
                _ => out.push_str(&format!("{line}\r\n")),
            }
        }
        if close {
            out.push_str("Connection: close\r\n");
        }
        out.push_str("\r\n");
        out
    }

    fn answer(&self, path: &str, body: Vec<u8>) -> Vec<u8> {
        if !body.starts_with(b"#EXTM3U") {
            return body;
        }
        let text = String::from_utf8_lossy(&body);
        let list = hls::read_playlist(&text);
        let key = path.split('?').next().unwrap_or(path).to_string();
        let mut next_parts = self.next_parts.lock().unwrap_or_else(|e| e.into_inner());
        match next_part(&text) {
            Some(next) => next_parts.insert(key, next),
            None => next_parts.remove(&key),
        };
        drop(next_parts);
        if !list.segments.is_empty() {
            self.note(path, Mark::of(&list));
        }
        let source = self.source(path);
        let mut out = String::with_capacity(text.len());
        for line in without_blocking_reload(&text).lines() {
            out.push_str(&with_local_uri(line, |uri| self.local(&source, uri)));
            out.push('\n');
        }
        out.into_bytes()
    }

    fn note(&self, path: &str, now: Mark) {
        let key = path.split('?').next().unwrap_or(path).to_string();
        let before = self
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, now.clone());
        if before.is_some_and(|before| !before.continued_by(&now)) {
            let _ = self.started_over.send(());
        }
    }
}

enum Passed {
    Through(bool),
    Playlist(Vec<u8>),
}

async fn ask_source(
    dialer: &Dialer,
    target: &Target<'_>,
    kept: Option<Box<dyn Conn>>,
    request: &str,
) -> io::Result<(Box<dyn Conn>, String, Vec<u8>)> {
    let timed_out = |what: String| io::Error::new(io::ErrorKind::TimedOut, what);
    let mut conn = match kept {
        Some(conn) => conn,
        None => {
            let to = target.address();
            tokio::time::timeout(FETCH_TIMEOUT, dialer.connect(target))
                .await
                .map_err(|_| timed_out(format!("no connection to {to} after {FETCH_TIMEOUT:?}")))?
                .map_err(|e| io::Error::new(e.kind(), format!("no connection to {to}: {e}")))?
        }
    };
    conn.write_all(request.as_bytes()).await?;
    let mut all = Vec::new();
    let head = async {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(end) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                return Ok(end);
            }
            if all.len() > MAX_HEAD {
                return Err(io::Error::other("an answer head without end"));
            }
            let n = conn.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            all.extend_from_slice(&chunk[..n]);
        }
    };
    let end = tokio::time::timeout(FETCH_TIMEOUT, head)
        .await
        .map_err(|_| timed_out(format!("connected, no answer after {FETCH_TIMEOUT:?}")))??;
    let early = all.split_off(end + 4);
    all.truncate(end);
    Ok((conn, String::from_utf8_lossy(&all).into_owned(), early))
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == name).then(|| value.trim())
    })
}

enum Body {
    Empty,
    Left(u64),
    Chunked(Chunks),
    ToClose,
}

enum Cut {
    Source(io::Error),
    Player,
}

impl Cut {
    fn error(self) -> io::Error {
        match self {
            Self::Source(e) => e,
            Self::Player => io::Error::from(io::ErrorKind::BrokenPipe),
        }
    }
}

impl Body {
    fn of(head: &str, head_only: bool) -> Self {
        let status = head
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        if head_only || status / 100 == 1 || status == 204 || status == 304 {
            return Self::Empty;
        }
        if header(head, "transfer-encoding").is_some_and(|v| v.contains("chunked")) {
            return Self::Chunked(Chunks::default());
        }
        match header(head, "content-length").and_then(|v| v.parse().ok()) {
            Some(len) => Self::Left(len),
            None => Self::ToClose,
        }
    }

    fn take(&mut self, bytes: &[u8]) -> io::Result<(usize, bool)> {
        Ok(match self {
            Self::Empty => (0, true),
            Self::Left(left) => {
                let n = bytes
                    .len()
                    .min(usize::try_from(*left).unwrap_or(usize::MAX));
                *left -= n as u64;
                (n, *left == 0)
            }
            Self::Chunked(chunks) => match chunks.end_in(bytes)? {
                Some(end) => (end, true),
                None => (bytes.len(), false),
            },
            Self::ToClose => (bytes.len(), false),
        })
    }

    async fn copy<W: AsyncWrite + Unpin>(
        mut self,
        early: &[u8],
        from: &mut Box<dyn Conn>,
        to: &mut W,
    ) -> Result<(), Cut> {
        let mut buf = vec![0u8; 64 * 1024];
        let mut piece = early;
        loop {
            let (len, ended) = self.take(piece).map_err(Cut::Source)?;
            to.write_all(&piece[..len]).await.map_err(|_| Cut::Player)?;
            if ended {
                return Ok(());
            }
            let n = from.read(&mut buf).await.map_err(Cut::Source)?;
            if n == 0 {
                return match self {
                    Self::ToClose => Ok(()),
                    _ => Err(Cut::Source(io::ErrorKind::UnexpectedEof.into())),
                };
            }
            piece = &buf[..n];
        }
    }
}

#[derive(Default)]
struct Chunks {
    state: ChunkState,
    line: Vec<u8>,
}

#[derive(Default)]
enum ChunkState {
    #[default]
    Size,
    Data(u64),
    DataEnd(u8),
    Trailer,
}

impl Chunks {
    fn end_in(&mut self, bytes: &[u8]) -> io::Result<Option<usize>> {
        let mut at = 0;
        while at < bytes.len() {
            match &mut self.state {
                ChunkState::Data(left) => {
                    let n = (bytes.len() - at).min(usize::try_from(*left).unwrap_or(usize::MAX));
                    at += n;
                    *left -= n as u64;
                    if *left == 0 {
                        self.state = ChunkState::DataEnd(2);
                    }
                }
                ChunkState::DataEnd(left) => {
                    at += 1;
                    *left -= 1;
                    if *left == 0 {
                        self.state = ChunkState::Size;
                    }
                }
                ChunkState::Size | ChunkState::Trailer => {
                    let byte = bytes[at];
                    at += 1;
                    if byte != b'\n' {
                        if self.line.len() > MAX_HEAD {
                            return Err(io::Error::other("a chunk line without end"));
                        }
                        self.line.push(byte);
                        continue;
                    }
                    let line = std::mem::take(&mut self.line);
                    let line = line.strip_suffix(b"\r").unwrap_or(&line);
                    if matches!(self.state, ChunkState::Trailer) {
                        if line.is_empty() {
                            return Ok(Some(at));
                        }
                        continue;
                    }
                    let size = std::str::from_utf8(line)
                        .ok()
                        .and_then(|l| l.split(';').next())
                        .and_then(|s| u64::from_str_radix(s.trim(), 16).ok())
                        .ok_or_else(|| io::Error::other("a chunk without size"))?;
                    self.state = if size == 0 {
                        ChunkState::Trailer
                    } else {
                        ChunkState::Data(size)
                    };
                }
            }
        }
        Ok(None)
    }
}

fn is_playlist(path: &str) -> bool {
    path.split('?').next().unwrap_or(path).ends_with(".m3u8")
}

fn with_local_uri(line: &str, local: impl Fn(&str) -> Option<String>) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return line.to_string();
    }
    if !trimmed.starts_with('#') {
        return local(trimmed).unwrap_or_else(|| line.to_string());
    }
    let attribute = line
        .match_indices("URI=\"")
        .find(|(at, _)| line[..*at].ends_with([':', ',']));
    let Some((at, key)) = attribute else {
        return line.to_string();
    };
    let start = at + key.len();
    let Some(len) = line[start..].find('"') else {
        return line.to_string();
    };
    match local(&line[start..start + len]) {
        Some(uri) => format!("{}{uri}{}", &line[..start], &line[start + len..]),
        None => line.to_string(),
    }
}

fn next_part(text: &str) -> Option<(u64, u64)> {
    let control = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("#EXT-X-SERVER-CONTROL:"))?;
    if !control
        .split(',')
        .any(|a| a.trim() == "CAN-BLOCK-RELOAD=YES")
    {
        return None;
    }
    let list = hls::read_playlist(text);
    let mut parts = 0;
    for line in text.lines().map(str::trim) {
        if line.starts_with("#EXT-X-PART:") {
            parts += 1;
        } else if !line.is_empty() && !line.starts_with('#') {
            parts = 0;
        }
    }
    Some((list.first + list.segments.len() as u64, parts))
}

fn without_blocking_reload(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("#EXT-X-SKIP") {
            continue;
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-SERVER-CONTROL:") {
            let kept: Vec<&str> = attrs
                .split(',')
                .filter(|a| !BLOCKING.iter().any(|b| a.trim().starts_with(b)))
                .collect();
            if !kept.is_empty() {
                out.push_str("#EXT-X-SERVER-CONTROL:");
                out.push_str(&kept.join(","));
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

async fn serve(upstream: Arc<Upstream>, mut sock: TcpStream) {
    let mut buf = Vec::new();
    while let Some(ask) = read_request(&mut sock, &mut buf).await {
        let fetched = if is_playlist(&ask.path) {
            upstream.fetch_playlist(&ask.path).await
        } else {
            match upstream.pass(&ask, &mut sock).await {
                Ok(Passed::Through(true)) => continue,
                Ok(Passed::Through(false)) => return,
                Ok(Passed::Playlist(list)) => Ok(list),
                Err(e) => Err(e),
            }
        };
        let (status, body) = match fetched {
            Ok(body) => ("200 OK", upstream.answer(&ask.path, body)),
            Err(e) => {
                println!("[hlsGateway] {}: {e}", ask.path);
                ("502 Bad Gateway", Vec::new())
            }
        };
        let kind = if is_playlist(&ask.path) || body.starts_with(b"#EXTM3U") {
            "application/vnd.apple.mpegurl"
        } else {
            "application/octet-stream"
        };
        let close = if ask.close {
            "Connection: close\r\n"
        } else {
            ""
        };
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
    close: bool,
    range: String,
}

async fn read_request<S: AsyncRead + Unpin>(sock: &mut S, buf: &mut Vec<u8>) -> Option<Request> {
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            buf.drain(..end + 4);
            let mut lines = head.lines();
            let mut words = lines.next()?.split(' ');
            let (method, path) = (words.next()?, words.next()?);
            let (mut close, mut range) = (false, String::new());
            for line in lines.map(|l| l.trim_end_matches('\r')) {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                match name.trim().to_ascii_lowercase().as_str() {
                    "connection" => close |= value.to_ascii_lowercase().contains("close"),
                    "range" | "if-range" => range.push_str(&format!("{line}\r\n")),
                    _ => {}
                }
            }
            let head_only = method == "HEAD";
            return Some(Request {
                path: path.to_string(),
                head_only,
                close,
                range,
            });
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
    use std::pin::Pin;
    use std::sync::Mutex as StdMutex;

    use openssl::ssl::Ssl;
    use tokio_openssl::SslStream;

    use crate::test_tls;

    const LIVE: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n\
        #EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=0.6\n\
        #EXT-X-PART-INF:PART-TARGET=0.2\n#EXT-X-MEDIA-SEQUENCE:5\n\
        #EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\nseg_5.m4s\n\
        #EXT-X-PART:DURATION=0.2,URI=\"part_6_0.m4s\"\n\
        #EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"part_6_1.m4s\"\n\
        #EXT-X-RENDITION-REPORT:URI=\"other.m3u8\",LAST-MSN=5\n";

    #[test]
    fn a_playlist_keeps_its_parts_without_the_blocking_reload() {
        assert_eq!(
            without_blocking_reload(LIVE),
            LIVE.replace("CAN-BLOCK-RELOAD=YES,", "")
        );
        assert_eq!(
            without_blocking_reload(
                "#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES\r\n#EXT-X-SKIP:SKIPPED-SEGMENTS=3\r\nseg.m4s\r\n"
            ),
            "seg.m4s\n"
        );
        assert_eq!(
            next_part(LIVE),
            Some((6, 1)),
            "the second part of segment 6 comes next"
        );
        assert_eq!(
            next_part("#EXT-X-MEDIA-SEQUENCE:5\nseg_5.m4s\n"),
            None,
            "no blocking offered"
        );
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
        dropped: Arc<StdMutex<usize>>,
        asked: Arc<StdMutex<Vec<String>>>,
    }

    async fn phone() -> Phone {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let playlist = Arc::new(StdMutex::new(LIVE.to_string()));
        let connections = Arc::new(StdMutex::new(0));
        let dropped = Arc::new(StdMutex::new(0));
        let asked = Arc::new(StdMutex::new(Vec::new()));
        let (list, count, drop) = (playlist.clone(), connections.clone(), dropped.clone());
        let log = asked.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                *count.lock().unwrap() += 1;
                let (list, drop, log) = (list.clone(), drop.clone(), log.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    while let Some(ask) = read_request(&mut sock, &mut buf).await {
                        if std::mem::replace(&mut *drop.lock().unwrap(), 0) > 0 {
                            return;
                        }
                        log.lock().unwrap().push(ask.path.clone());
                        let body = match ask.path.split('?').next().unwrap_or_default() {
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
        Phone {
            base,
            playlist,
            connections,
            dropped,
            asked,
        }
    }

    async fn ask(url: &str, extra: &str) -> String {
        let Target { host, path, .. } = Target::of(url).unwrap();
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
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "));
                len.and_then(|v| v.parse::<usize>().ok())
                    .is_some_and(|len| body.len() >= len)
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
        let gateway = Gateway::open(
            Client::default(),
            &format!("{}/live/index.m3u8", phone.base),
            tx,
        )
        .await
        .unwrap();
        assert!(gateway.url().starts_with("http://127.0.0.1:"));
        assert!(gateway.url().ends_with("/live/index.m3u8"));

        let list = ask(gateway.url(), "").await;
        assert!(list.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(list.contains("Content-Type: application/vnd.apple.mpegurl"));
        assert!(
            list.contains("EXT-X-PART:") && !list.contains("CAN-BLOCK-RELOAD"),
            "{list}"
        );
        ask(gateway.url(), "").await;
        let asked: Vec<String> = phone
            .asked
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains("index"))
            .cloned()
            .collect();
        assert_eq!(
            asked,
            [
                "/live/index.m3u8",
                "/live/index.m3u8?_HLS_msn=6&_HLS_part=1"
            ],
            "the gateway waits upstream for the next part"
        );

        let segment = gateway.url().replace("index.m3u8", "seg_5.m4s");
        for _ in 0..3 {
            assert!(ask(&segment, "").await.ends_with("\r\n\r\nsegment five"));
        }
        assert_eq!(*phone.connections.lock().unwrap(), 1);

        let missing = ask(&gateway.url().replace("index.m3u8", "gone.m4s"), "").await;
        assert!(
            missing.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "{missing}"
        );
    }

    #[tokio::test]
    async fn a_stream_that_starts_over_is_reported_once() {
        let phone = phone().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(
            Client::default(),
            &format!("{}/live/index.m3u8", phone.base),
            tx,
        )
        .await
        .unwrap();
        ask(gateway.url(), "").await;
        ask(gateway.url(), "").await;
        assert!(rx.try_recv().is_err());

        let turned = LIVE.replace("init.mp4", "init_1.mp4");
        *phone.playlist.lock().unwrap() = turned;
        ask(gateway.url(), "").await;
        ask(gateway.url(), "").await;
        assert!(rx.try_recv().is_ok());
        assert!(
            rx.try_recv().is_err(),
            "the new stream is the one compared against now"
        );
    }

    #[tokio::test]
    async fn a_failed_playlist_is_asked_again_and_head_and_close_are_honoured() {
        let phone = phone().await;
        let (tx, _rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(
            Client::default(),
            &format!("{}/live/index.m3u8", phone.base),
            tx,
        )
        .await
        .unwrap();
        *phone.dropped.lock().unwrap() = 1;
        assert!(
            ask(gateway.url(), "")
                .await
                .starts_with("HTTP/1.1 200 OK\r\n")
        );

        let head = ask(gateway.url(), "Connection: close\r\n").await;
        assert!(head.contains("Connection: close\r\n"));
        let Target { host, path, .. } = Target::of(gateway.url()).unwrap();
        let mut sock = TcpStream::connect(host).await.unwrap();
        sock.write_all(format!("HEAD {path} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut answer = vec![0u8; 4096];
        let n = sock.read(&mut answer).await.unwrap();
        let answer = String::from_utf8_lossy(&answer[..n]).to_string();
        assert!(answer.starts_with("HTTP/1.1 200 OK") && answer.ends_with("\r\n\r\n"));

        let mut endless = TcpStream::connect(host).await.unwrap();
        let _ = endless.write_all(&vec![b'x'; MAX_HEAD + 4096]).await;
        let read = endless.read(&mut [0u8; 16]).await;
        assert!(
            matches!(read, Ok(0) | Err(_)),
            "an endless head gets no answer"
        );
    }

    fn upstream(origin: &str) -> Upstream {
        Upstream {
            origin: origin.into(),
            client: Mutex::new(Client::default()),
            seen: std::sync::Mutex::new(HashMap::new()),
            started_over: mpsc::unbounded_channel().0,
            next_parts: std::sync::Mutex::default(),
        }
    }

    #[test]
    fn every_uri_of_a_playlist_points_back_at_the_gateway() {
        let up = upstream("https://cdn.example");
        assert_eq!(
            up.source("/v/index.m3u8"),
            "https://cdn.example/v/index.m3u8"
        );
        assert_eq!(up.source("/~/http/h:8080/a?b=1"), "http://h:8080/a?b=1");

        let list = "#EXTM3U\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example/k?id=1\",IV=0x1\n\
            #EXT-X-MAP:URI=\"/v/init.mp4\"\n#EXTINF:4,\nseg_1.m4s\n\
            #EXTINF:4,\nhttps://cdn.example/v/seg_2.m4s\n\
            #EXTINF:4,\nhttp://other.example:8080/seg_3.ts\n\
            #EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"skd://key\"\n";
        let out = String::from_utf8(up.answer("/v/index.m3u8", list.into())).unwrap();
        assert_eq!(
            out,
            "#EXTM3U\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"/~/https/keys.example/k?id=1\",IV=0x1\n\
             #EXT-X-MAP:URI=\"/v/init.mp4\"\n#EXTINF:4,\nseg_1.m4s\n\
             #EXTINF:4,\n/v/seg_2.m4s\n\
             #EXTINF:4,\n/~/http/other.example:8080/seg_3.ts\n\
             #EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"skd://key\"\n"
        );

        let media =
            "#EXTM3U\n#EXTINF:4,\n/a/seg.ts\n#EXTINF:4,\nseg.ts\n#EXTINF:4,\n//cdn.example/x.ts\n";
        let out = up.answer("/~/http/other.example/a/list", media.into());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "#EXTM3U\n#EXTINF:4,\n/~/http/other.example/a/seg.ts\n#EXTINF:4,\nseg.ts\n\
             #EXTINF:4,\n/~/http/cdn.example/x.ts\n"
        );
        assert_eq!(
            with_local_uri("#EXT-X-X:XURI=\"/a\"", |_| Some("b".into())),
            "#EXT-X-X:XURI=\"/a\""
        );
        assert_eq!(
            with_local_uri("#EXT-X-MAP:URI=\"/a", |_| Some("b".into())),
            "#EXT-X-MAP:URI=\"/a"
        );
    }

    #[tokio::test]
    async fn an_https_source_reaches_the_player_as_plain_http() {
        let (acceptor, trust) = test_tls::ca();
        let plain = phone().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let list = format!(
            "#EXTM3U\n#EXTINF:1,\n{}/live/seg_5.m4s\n#EXTINF:1,\nseg.ts\n",
            plain.base
        );
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                let ssl = Ssl::new(acceptor.context()).unwrap();
                let list = list.clone();
                tokio::spawn(async move {
                    let mut tls = SslStream::new(ssl, tcp).unwrap();
                    if Pin::new(&mut tls).accept().await.is_err() {
                        return;
                    }
                    let mut buf = Vec::new();
                    while let Some(ask) = read_request(&mut tls, &mut buf).await {
                        let (kind, body) = match ask.path.as_str() {
                            "/cdn/play" => (
                                "Content-Type: application/vnd.apple.mpegurl\r\n",
                                list.clone().into_bytes(),
                            ),
                            _ => ("", b"tls segment".to_vec()),
                        };
                        let head = format!(
                            "HTTP/1.1 200 OK\r\n{kind}Content-Length: {}\r\n\r\n",
                            body.len()
                        );
                        if tls.write_all(head.as_bytes()).await.is_err()
                            || tls.write_all(&body).await.is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });

        let playlist = format!("https://127.0.0.1:{port}/cdn/play");
        let (tx, _rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(Client::default().trusting(trust), &playlist, tx)
            .await
            .unwrap();
        assert!(
            gateway.url().starts_with("http://127.0.0.1:") && gateway.url().ends_with("/cdn/play")
        );
        let answer = ask(gateway.url(), "").await;
        assert!(
            answer.contains("Content-Type: application/vnd.apple.mpegurl"),
            "{answer}"
        );
        let elsewhere = plain.base.replace("http://", "/~/http/") + "/live/seg_5.m4s";
        assert!(answer.contains(&format!("\n{elsewhere}\n")), "{answer}");

        let local = gateway.url().trim_end_matches("/cdn/play").to_string();
        assert!(
            ask(&format!("{local}{elsewhere}"), "")
                .await
                .ends_with("\r\n\r\nsegment five")
        );
        let segment = ask(&gateway.url().replace("play", "seg.ts"), "").await;
        assert!(segment.ends_with("\r\n\r\ntls segment"));
    }

    #[tokio::test]
    async fn a_source_out_of_reach_fails_in_the_player() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let gateway = Gateway::open(Client::default(), "skd://key/a", tx)
            .await
            .unwrap();
        assert!(gateway.url().ends_with("/~/skd/key/a"));
        assert!(
            ask(gateway.url(), "")
                .await
                .starts_with("HTTP/1.1 502 Bad Gateway\r\n")
        );
    }
}
