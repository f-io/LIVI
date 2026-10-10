//! Plays an HLS stream of fMP4 segments with H.264 video, the kind a video app on the phone
//! serves, as length-prefixed access units paced at their own rate.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;

const FETCH_TIMEOUT: Duration = Duration::from_secs(4);
const NAL_LENGTH: usize = 4;

/// The video track of an init segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: u32,
    pub timescale: u32,
    /// The AVCDecoderConfigurationRecord.
    pub avcc: Vec<u8>,
    pub width: u32,
    pub height: u32,
    default_duration: u32,
    default_size: u32,
}

/// One access unit, its NALs with four byte lengths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub data: Vec<u8>,
    pub duration: u32,
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |acc, b| (acc << 8) | u64::from(*b))
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    Some(be(buf.get(at..at + 4)?) as u32)
}

/// The boxes of one level, each as (type, body, offset of the box in `buf`).
fn boxes(buf: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8], usize)> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let size = u32_at(buf, at)? as usize;
        let kind: [u8; 4] = buf.get(at + 4..at + 8)?.try_into().ok()?;
        let (head, size) = match size {
            0 => (8, buf.len() - at),
            1 => (16, usize::try_from(be(buf.get(at + 8..at + 16)?)).ok()?),
            n => (8, n),
        };
        let body = buf.get(at + head..at.checked_add(size)?)?;
        let start = at;
        at += size;
        Some((kind, body, start))
    })
}

fn child<'a>(buf: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(buf).find(|(k, _, _)| k == kind).map(|(_, body, _)| body)
}

fn path<'a>(buf: &'a [u8], kinds: &[&[u8; 4]]) -> Option<&'a [u8]> {
    kinds.iter().try_fold(buf, |at, kind| child(at, kind))
}

fn video_track(trak: &[u8]) -> Option<Track> {
    let mdia = child(trak, b"mdia")?;
    if child(mdia, b"hdlr")?.get(8..12)? != b"vide" {
        return None;
    }
    let mdhd = child(mdia, b"mdhd")?;
    let timescale = if mdhd.first()? == &1 { u32_at(mdhd, 20)? } else { u32_at(mdhd, 12)? };
    let stsd = path(mdia, &[b"minf", b"stbl", b"stsd"])?;
    let (kind, entry, _) = boxes(stsd.get(8..)?).next()?;
    if &kind != b"avc1" && &kind != b"avc3" {
        return None;
    }
    // The visual sample entry is 78 bytes before its own boxes, the size sits at 24.
    let width = be(entry.get(24..26)?) as u32;
    let height = be(entry.get(26..28)?) as u32;
    let avcc = child(entry.get(78..)?, b"avcC")?.to_vec();
    if usize::from(avcc.get(4)? & 3) + 1 != NAL_LENGTH {
        return None;
    }
    let tkhd = child(trak, b"tkhd")?;
    let id = if tkhd.first()? == &1 { u32_at(tkhd, 20)? } else { u32_at(tkhd, 12)? };
    Some(Track { id, timescale, avcc, width, height, default_duration: 0, default_size: 0 })
}

/// The first H.264 track with four byte NAL lengths, and its defaults from `trex`.
pub fn read_init(init: &[u8]) -> Option<Track> {
    let moov = child(init, b"moov")?;
    let mut track =
        boxes(moov).filter(|(k, _, _)| k == b"trak").find_map(|(_, trak, _)| video_track(trak))?;
    if let Some(mvex) = child(moov, b"mvex") {
        for (_, trex, _) in boxes(mvex).filter(|(k, _, _)| k == b"trex") {
            if u32_at(trex, 4) == Some(track.id) {
                track.default_duration = u32_at(trex, 12).unwrap_or(0);
                track.default_size = u32_at(trex, 16).unwrap_or(0);
            }
        }
    }
    Some(track)
}

const TFHD_BASE_OFFSET: u32 = 0x1;
const TFHD_DESCRIPTION: u32 = 0x2;
const TFHD_DURATION: u32 = 0x8;
const TFHD_SIZE: u32 = 0x10;
const TRUN_DATA_OFFSET: u32 = 0x1;
const TRUN_FIRST_FLAGS: u32 = 0x4;
const TRUN_DURATION: u32 = 0x100;
const TRUN_SIZE: u32 = 0x200;
const TRUN_FLAGS: u32 = 0x400;
const TRUN_CTO: u32 = 0x800;

/// The samples of `track` in a media segment, in decode order.
pub fn read_segment(segment: &[u8], track: &Track) -> Vec<Sample> {
    let mut samples = Vec::new();
    for (kind, moof, moof_at) in boxes(segment) {
        if &kind != b"moof" {
            continue;
        }
        for (_, traf, _) in boxes(moof).filter(|(k, _, _)| k == b"traf") {
            read_traf(segment, moof_at, traf, track, &mut samples);
        }
    }
    samples
}

fn read_traf(segment: &[u8], moof_at: usize, traf: &[u8], track: &Track, out: &mut Vec<Sample>) {
    let Some(tfhd) = child(traf, b"tfhd") else { return };
    let (Some(flags), Some(id)) = (u32_at(tfhd, 0), u32_at(tfhd, 4)) else { return };
    if id != track.id {
        return;
    }
    let flags = flags & 0x00FF_FFFF;
    let mut at = 8;
    let mut base = moof_at;
    if flags & TFHD_BASE_OFFSET != 0 {
        base = tfhd.get(at..at + 8).map_or(moof_at, |b| be(b) as usize);
        at += 8;
    }
    if flags & TFHD_DESCRIPTION != 0 {
        at += 4;
    }
    let mut duration = track.default_duration;
    if flags & TFHD_DURATION != 0 {
        duration = u32_at(tfhd, at).unwrap_or(duration);
        at += 4;
    }
    let size = if flags & TFHD_SIZE != 0 { u32_at(tfhd, at) } else { None };
    let size = size.unwrap_or(track.default_size);
    let mut next = base;
    for (_, trun, _) in boxes(traf).filter(|(k, _, _)| k == b"trun") {
        next = read_trun(segment, base, next, trun, (duration, size), out);
    }
}

/// Returns where the data of a following run without its own offset starts.
fn read_trun(
    segment: &[u8],
    base: usize,
    mut data: usize,
    trun: &[u8],
    (default_duration, default_size): (u32, u32),
    out: &mut Vec<Sample>,
) -> usize {
    let (Some(flags), Some(count)) = (u32_at(trun, 0), u32_at(trun, 4)) else { return data };
    let mut at = 8;
    if flags & TRUN_DATA_OFFSET != 0 {
        let Some(offset) = u32_at(trun, at) else { return data };
        data = base.wrapping_add_signed(offset as i32 as isize);
        at += 4;
    }
    if flags & TRUN_FIRST_FLAGS != 0 {
        at += 4;
    }
    for _ in 0..count {
        let mut field = |bit: u32, default: u32| {
            if flags & bit == 0 {
                return Some(default);
            }
            let v = u32_at(trun, at);
            at += 4;
            v
        };
        let (Some(duration), Some(size)) =
            (field(TRUN_DURATION, default_duration), field(TRUN_SIZE, default_size))
        else {
            return data;
        };
        let _ = field(TRUN_FLAGS, 0);
        let _ = field(TRUN_CTO, 0);
        let Some(bytes) = segment.get(data..data + size as usize) else { return data };
        out.push(Sample { data: bytes.to_vec(), duration });
        data += size as usize;
    }
    data
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    pub init: Option<String>,
    pub first: u64,
    pub segments: Vec<String>,
    pub discontinuity: u64,
    /// The server holds a playlist request until the asked segment exists.
    pub block_reload: bool,
}

/// Whole segments only, the parts of a low-latency playlist are left out.
pub fn read_playlist(text: &str) -> Playlist {
    let mut list = Playlist::default();
    for line in text.lines().map(str::trim) {
        if let Some(seq) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            list.first = seq.parse().unwrap_or(0);
        } else if let Some(seq) = line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
            list.discontinuity = seq.parse().unwrap_or(0);
        } else if let Some(control) = line.strip_prefix("#EXT-X-SERVER-CONTROL:") {
            list.block_reload = control.split(',').any(|a| a.trim() == "CAN-BLOCK-RELOAD=YES");
        } else if let Some(map) = line.strip_prefix("#EXT-X-MAP:") {
            list.init = map
                .split("URI=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string);
        } else if !line.is_empty() && !line.starts_with('#') {
            list.segments.push(line.to_string());
        }
    }
    list
}

/// `uri` as seen from the playlist at `base`.
fn resolve(base: &str, uri: &str) -> String {
    if uri.contains("://") {
        return uri.to_string();
    }
    let origin_end = base.find("://").map_or(0, |at| at + 3);
    let host_end = base[origin_end..].find('/').map_or(base.len(), |at| origin_end + at);
    if uri.starts_with('/') {
        return format!("{}{uri}", &base[..host_end]);
    }
    let path = base.split(['?', '#']).next().unwrap_or(base);
    let dir_end = path.rfind('/').filter(|&at| at >= host_end).map_or(path.len(), |at| at + 1);
    let sep = if dir_end == path.len() && !path.ends_with('/') { "/" } else { "" };
    format!("{}{sep}{uri}", &path[..dir_end])
}

/// A single GET on a connection of its own.
pub async fn get(url: &str) -> io::Result<Vec<u8>> {
    Client::default().get(url).await
}

/// Host and path of a plain http URL.
pub(crate) fn split_url(url: &str) -> io::Result<(&str, &str)> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not an http URL"))?;
    Ok(rest.find('/').map_or((rest, "/"), |at| (&rest[..at], &rest[at..])))
}

/// One HTTP/1.1 connection kept open between requests. Behind the tunnel on 127.0.0.1 every new
/// connection costs a handshake with the phone, one per playlist and segment was too many.
#[derive(Default)]
pub struct Client {
    open: Option<(String, TcpStream)>,
}

impl Client {
    pub async fn get(&mut self, url: &str) -> io::Result<Vec<u8>> {
        let (host, path) = split_url(url)?;
        let reused = self.open.as_ref().is_some_and(|(h, _)| h == host);
        if !reused {
            self.open = None;
        }
        match self.exchange(host, path, url).await {
            // A server may close a connection that sat idle, that one gets a fresh try.
            Err(e) if reused && e.kind() != io::ErrorKind::TimedOut => {
                self.open = None;
                self.exchange(host, path, url).await
            }
            other => other,
        }
    }

    async fn exchange(&mut self, host: &str, path: &str, url: &str) -> io::Result<Vec<u8>> {
        let open = self.open.take();
        let fetch = async {
            let mut sock = match open {
                Some((_, sock)) => sock,
                None => TcpStream::connect(host).await?,
            };
            sock.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
                .await?;
            let (head, body, again) = read_answer(&mut sock, url).await?;
            Ok::<_, io::Error>((head, body, again.then_some(sock)))
        };
        let (head, body, sock) = tokio::time::timeout(FETCH_TIMEOUT, fetch)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, format!("{url}: timed out")))??;
        if let Some(sock) = sock {
            self.open = Some((host.to_string(), sock));
        }
        if head.split(' ').nth(1) != Some("200") {
            let line = head.lines().next().unwrap_or_default().to_string();
            return Err(io::Error::other(format!("{url}: {line}")));
        }
        Ok(body)
    }
}

/// One answer off `sock`: the head in lower case, the body, and whether the connection can carry
/// another request.
async fn read_answer(sock: &mut TcpStream, url: &str) -> io::Result<(String, Vec<u8>, bool)> {
    let mut all = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut ended = false;
    loop {
        if let Some((head, body)) = split_answer(&all) {
            let again = !head.contains("connection: close");
            if let Some(len) = content_length(&head) {
                if body.len() >= len {
                    return Ok((head, body[..len].to_vec(), again));
                }
            } else if head.contains("transfer-encoding: chunked") {
                if let Some(out) = dechunk(body) {
                    return Ok((head, out, again));
                }
            } else if ended {
                // Without a length the body runs to the end of the connection.
                return Ok((head, body.to_vec(), false));
            }
            if ended {
                return Err(io::Error::other(format!("{url}: answer cut short")));
            }
        } else if ended {
            return Err(io::Error::other(format!("{url}: no HTTP answer")));
        }
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            ended = true;
        }
        all.extend_from_slice(&chunk[..n]);
    }
}

fn with_query(url: &str, query: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{query}")
}

/// The head in lower case and the body behind it.
fn split_answer(all: &[u8]) -> Option<(String, &[u8])> {
    let end = all.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some((String::from_utf8_lossy(&all[..end]).to_ascii_lowercase(), &all[end + 4..]))
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok())
}

fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|w| w == b"\r\n")?;
        let size = std::str::from_utf8(&body[..line_end]).ok()?.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        body = body.get(line_end + 2..)?;
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(..size)?);
        body = body.get(size + 2..)?;
    }
}

#[derive(Debug)]
struct StartedOver;

impl std::fmt::Display for StartedOver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the stream started over")
    }
}

impl std::error::Error for StartedOver {}

/// The server began the stream anew, with another init segment, numbering or discontinuity, so
/// it has to be opened again, maybe in another size.
pub fn started_over(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<StartedOver>())
}

/// A live stream whose init segment is read, ready to run.
pub struct Stream {
    url: String,
    pub track: Track,
    next: u64,
    give_up: Duration,
    client: Client,
    init: String,
    discontinuity: u64,
    block_reload: bool,
}

/// How long the playlist is left alone when it had nothing new.
const POLL: Duration = Duration::from_millis(250);
/// Fetching keeps retrying for this long before the stream counts as gone.
const GIVE_UP: Duration = Duration::from_secs(10);
/// Segments behind the newest the stream starts at, room for a slow fetch.
const START_BEHIND: u64 = 3;
/// Whole segments fetched ahead of the one playing.
const AHEAD: usize = 3;
/// A segment that takes this long to fetch is logged, with 1 s segments it eats the buffer.
const SLOW_FETCH: Duration = Duration::from_millis(500);

impl Stream {
    pub async fn open(url: &str) -> io::Result<Self> {
        let mut client = Client::default();
        let list = read_playlist(&String::from_utf8_lossy(&client.get(url).await?));
        let init = list.init.clone().ok_or_else(|| io::Error::other("no init segment"))?;
        let track = read_init(&client.get(&resolve(url, &init)).await?)
            .ok_or_else(|| io::Error::other("no H.264 track with four byte NAL lengths"))?;
        let newest = list.first + list.segments.len() as u64;
        Ok(Self {
            url: url.to_string(),
            track,
            next: newest.saturating_sub(START_BEHIND).max(list.first),
            give_up: GIVE_UP,
            client,
            init,
            discontinuity: list.discontinuity,
            block_reload: list.block_reload,
        })
    }

    /// Sends every access unit at its time until `frames` closes or fetching fails for good.
    pub async fn run(self, frames: mpsc::Sender<Vec<u8>>) -> io::Result<()> {
        let timescale = f64::from(self.track.timescale.max(1));
        let (segments, mut fetched) = mpsc::channel::<Vec<Sample>>(AHEAD);
        let play = async move {
            let mut due = Instant::now();
            while let Some(samples) = fetched.recv().await {
                // A late segment is shown at once instead of catching up in a rush.
                due = due.max(Instant::now());
                for sample in samples {
                    tokio::time::sleep_until(due).await;
                    if frames.send(sample.data).await.is_err() {
                        return;
                    }
                    due += Duration::from_secs_f64(f64::from(sample.duration) / timescale);
                }
            }
        };
        tokio::select! {
            fetched = self.fetch(segments) => fetched,
            () = play => Ok(()),
        }
    }

    async fn fetch(mut self, segments: mpsc::Sender<Vec<Sample>>) -> io::Result<()> {
        let mut failing: Option<Instant> = None;
        loop {
            match self.fetch_next().await {
                Ok(Some(samples)) => {
                    failing = None;
                    if segments.send(samples).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                Ok(None) => failing = None,
                Err(e) if started_over(&e) => return Err(e),
                Err(e) => {
                    let since = *failing.get_or_insert_with(Instant::now);
                    if since.elapsed() >= self.give_up {
                        return Err(e);
                    }
                    println!("[hls] {e}, trying again");
                }
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// The samples of the next segment, None while the playlist has none yet.
    async fn fetch_next(&mut self) -> io::Result<Option<Vec<Sample>>> {
        let list = self.playlist().await?;
        let newest = list.first + list.segments.len() as u64;
        if list.init.as_ref().is_some_and(|init| *init != self.init)
            || list.discontinuity != self.discontinuity
            || self.next > newest + 1
        {
            return Err(io::Error::other(StartedOver));
        }
        if self.next < list.first {
            self.next = list.first;
        }
        let at = usize::try_from(self.next - list.first).unwrap_or(usize::MAX);
        let Some(uri) = list.segments.get(at) else { return Ok(None) };
        let asked = Instant::now();
        let segment = self.client.get(&resolve(&self.url, uri)).await?;
        let took = asked.elapsed();
        self.next += 1;
        if took >= SLOW_FETCH {
            println!(
                "[hls] {uri}: {} KB in {} ms, {} behind the newest",
                segment.len() / 1024,
                took.as_millis(),
                newest.saturating_sub(self.next)
            );
        }
        Ok(Some(read_segment(&segment, &self.track)))
    }

    /// Where the server can hold the answer until the next segment exists, nothing polls. A held
    /// request it refuses falls back to the plain playlist.
    async fn playlist(&mut self) -> io::Result<Playlist> {
        if self.block_reload {
            let ask = with_query(&self.url, &format!("_HLS_msn={}", self.next));
            if let Ok(body) = self.client.get(&ask).await {
                let list = read_playlist(&String::from_utf8_lossy(&body));
                self.block_reload = list.block_reload;
                return Ok(list);
            }
        }
        let list = read_playlist(&String::from_utf8_lossy(&self.client.get(&self.url).await?));
        self.block_reload = list.block_reload;
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tokio::net::TcpListener;

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(body);
        b
    }

    fn full(version_flags: u32, rest: &[u8]) -> Vec<u8> {
        let mut b = version_flags.to_be_bytes().to_vec();
        b.extend_from_slice(rest);
        b
    }

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    const AVCC: [u8; 11] = [1, 0x64, 0, 0x1f, 0xFF, 0xE1, 0, 2, 0x67, 0x64, 0];

    fn trak(id: u32, handler: &[u8; 4], entry: &[u8; 4], avcc: &[u8], v1: bool) -> Vec<u8> {
        let tkhd = if v1 {
            full(1 << 24, &[vec![0; 16], words(&[id])].concat())
        } else {
            full(0, &[vec![0; 8], words(&[id])].concat())
        };
        let mdhd = if v1 {
            full(1 << 24, &[vec![0; 16], words(&[90_000])].concat())
        } else {
            full(0, &[vec![0; 8], words(&[90_000])].concat())
        };
        let hdlr = full(0, &[vec![0; 4], handler.to_vec()].concat());
        let mut sample_entry = vec![0u8; 78];
        sample_entry[24..26].copy_from_slice(&720u16.to_be_bytes());
        sample_entry[26..28].copy_from_slice(&576u16.to_be_bytes());
        sample_entry.extend(mp4_box(b"avcC", avcc));
        let stsd = full(0, &[words(&[1]), mp4_box(entry, &sample_entry)].concat());
        let stbl = mp4_box(b"stbl", &mp4_box(b"stsd", &stsd));
        let minf = mp4_box(b"minf", &stbl);
        let mdia =
            mp4_box(b"mdia", &[mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr), minf].concat());
        mp4_box(b"trak", &[mp4_box(b"tkhd", &tkhd), mdia].concat())
    }

    fn init() -> Vec<u8> {
        let trex = full(0, &words(&[2, 1, 3600, 5, 0]));
        let other_trex = full(0, &words(&[9, 1, 1, 1, 0]));
        let moov = [
            trak(1, b"soun", b"mp4a", &AVCC, false),
            trak(2, b"vide", b"avc1", &AVCC, true),
            mp4_box(b"mvex", &[mp4_box(b"trex", &other_trex), mp4_box(b"trex", &trex)].concat()),
        ]
        .concat();
        [mp4_box(b"ftyp", b"iso6"), mp4_box(b"moov", &moov)].concat()
    }

    fn track() -> Track {
        read_init(&init()).unwrap()
    }

    #[test]
    fn the_init_segment_names_the_video_track() {
        let t = track();
        assert_eq!((t.id, t.timescale, t.width, t.height), (2, 90_000, 720, 576));
        assert_eq!(t.avcc, AVCC);
        assert_eq!((t.default_duration, t.default_size), (3600, 5));

        let short_lengths = [1, 0x64, 0, 0x1f, 0xFD, 0xE1, 0, 2, 0x67, 0x64, 0];
        let moov = mp4_box(b"moov", &trak(1, b"vide", b"avc1", &short_lengths, false));
        assert_eq!(read_init(&moov), None);
        let hevc = mp4_box(b"moov", &trak(1, b"vide", b"hvc1", &AVCC, false));
        assert_eq!(read_init(&hevc), None);
        let only = mp4_box(b"moov", &trak(4, b"vide", b"avc3", &AVCC, false));
        let t = read_init(&only).unwrap();
        assert_eq!((t.id, t.default_duration), (4, 0));
        assert_eq!(read_init(b"\0\0\0\x10moov"), None);
    }

    fn moof(
        track: u32,
        tfhd_flags: u32,
        tfhd_rest: &[u32],
        trun_flags: u32,
        trun: &[u32],
    ) -> Vec<u8> {
        let tfhd = full(tfhd_flags, &words(&[&[track], tfhd_rest].concat()));
        let trun = full(trun_flags, &words(trun));
        let traf = mp4_box(b"traf", &[mp4_box(b"tfhd", &tfhd), mp4_box(b"trun", &trun)].concat());
        mp4_box(b"moof", &traf)
    }

    #[test]
    fn a_segment_yields_its_samples_from_mdat() {
        let t = track();
        let payload = b"AAAABBBCC".to_vec();
        // Durations and sizes per sample, the offset counted from the moof.
        let probe = moof(
            2,
            0x020000,
            &[],
            0x301 | 0x400 | 0x800,
            &[3, 0, 1, 4, 0, 0, 2, 3, 0, 0, 3, 2, 0, 0],
        );
        let offset = probe.len() as u32 + 8;
        let first = moof(
            2,
            0x020000,
            &[],
            0x301 | 0x400 | 0x800,
            &[3, offset, 1, 4, 0, 0, 2, 3, 0, 0, 3, 2, 0, 0],
        );
        let segment = [first, mp4_box(b"mdat", &payload)].concat();
        let samples = read_segment(&segment, &t);
        let got: Vec<(&[u8], u32)> =
            samples.iter().map(|s| (s.data.as_slice(), s.duration)).collect();
        assert_eq!(got, [(&b"AAAA"[..], 1), (b"BBB", 2), (b"CC", 3)]);

        // Defaults from tfhd, then from trex, no offset means right behind the moof.
        let probe = moof(2, 0x18, &[10, 2], 0x4, &[2, 0]);
        let segment = [probe, b"XXYY".to_vec()].concat();
        let samples = read_segment(&segment, &t);
        assert_eq!(samples.len(), 2);
        assert_eq!((samples[0].data.len(), samples[1].duration), (2, 10));

        let base = moof(2, 0x1 | 0x2, &[0, 0, 1], 0x201, &[1, 0, 4]);
        let at = base.len();
        let base = moof(2, 0x1 | 0x2, &[0, at as u32, 1], 0x201, &[1, 0, 4]);
        let segment = [base, b"WXYZ".to_vec()].concat();
        let samples = read_segment(&segment, &t);
        assert_eq!((samples[0].data.as_slice(), samples[0].duration), (&b"WXYZ"[..], 3600));

        let other = moof(7, 0, &[], 0x200, &[1, 4]);
        assert!(read_segment(&[other, b"WXYZ".to_vec()].concat(), &t).is_empty());
        let past_end = moof(2, 0, &[], 0x200, &[1, 400]);
        assert!(read_segment(&past_end, &t).is_empty());
        let cut = moof(2, 0, &[], 0x300, &[2, 1]);
        assert!(read_segment(&cut, &t).is_empty());
        let no_offset = moof(2, 0, &[], 0x1, &[1]);
        assert!(read_segment(&no_offset, &t).is_empty());
        let no_tfhd = mp4_box(b"moof", &mp4_box(b"traf", &[]));
        assert!(read_segment(&no_tfhd, &t).is_empty());
        let wide =
            [1u32.to_be_bytes().to_vec(), b"mdat".to_vec(), 16u64.to_be_bytes().to_vec()].concat();
        let rest =
            [wide, mp4_box(b"free", b""), 0u32.to_be_bytes().to_vec(), b"skip".to_vec()].concat();
        assert!(read_segment(&rest, &t).is_empty());
    }

    #[test]
    fn the_playlist_lists_whole_segments() {
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:108\n#EXT-X-MAP:URI=\"init_0.mp4\"\n\
                    #EXTINF:1.000,\nseg_108.m4s\n#EXT-X-PART:DURATION=0.2,URI=\"part_109_0.m4s\"\n\
                    #EXTINF:1.000,\nseg_109.m4s\n";
        let list = read_playlist(text);
        assert_eq!(list.init.as_deref(), Some("init_0.mp4"));
        assert_eq!(list.first, 108);
        assert_eq!(list.segments, ["seg_108.m4s", "seg_109.m4s"]);
        assert_eq!((list.discontinuity, list.block_reload), (0, false));
        assert_eq!(read_playlist("#EXT-X-MEDIA-SEQUENCE:x"), Playlist::default());

        let live = read_playlist(
            "#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=0.468\n\
             #EXT-X-DISCONTINUITY-SEQUENCE:2\n",
        );
        assert_eq!((live.discontinuity, live.block_reload), (2, true));
        assert!(!read_playlist("#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=NO\n").block_reload);
        assert_eq!(with_query("http://h/a.m3u8", "x=1"), "http://h/a.m3u8?x=1");
        assert_eq!(with_query("http://h/a.m3u8?y=2", "x=1"), "http://h/a.m3u8?y=2&x=1");
    }

    #[test]
    fn uris_resolve_against_the_playlist() {
        let base = "http://127.0.0.1:9/mirror/index.m3u8?x=1";
        assert_eq!(resolve(base, "seg_1.m4s"), "http://127.0.0.1:9/mirror/seg_1.m4s");
        assert_eq!(resolve(base, "/other/a.mp4"), "http://127.0.0.1:9/other/a.mp4");
        assert_eq!(resolve(base, "http://h/a"), "http://h/a");
        assert_eq!(resolve("http://127.0.0.1:9", "a"), "http://127.0.0.1:9/a");
        assert_eq!(resolve("http://127.0.0.1:9/", "a"), "http://127.0.0.1:9/a");
    }

    #[test]
    fn chunked_bodies_are_joined() {
        assert_eq!(dechunk(b"3\r\nabc\r\n2;x\r\nde\r\n0\r\n\r\n"), Some(b"abcde".to_vec()));
        assert_eq!(dechunk(b"zz\r\n"), None);
        assert_eq!(dechunk(b"5\r\nab"), None);
    }

    /// Serves `files` by path, answering anything else with 404.
    async fn server(files: Vec<(&'static str, Vec<u8>, bool)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let files = files.clone();
                tokio::spawn(async move {
                    let mut ask = vec![0u8; 1024];
                    let n = sock.read(&mut ask).await.unwrap_or(0);
                    let ask = String::from_utf8_lossy(&ask[..n]).to_string();
                    let path = ask.split(' ').nth(1).unwrap_or_default().to_string();
                    let answer = match files.iter().find(|(p, _, _)| *p == path) {
                        Some((_, body, true)) => {
                            let mut a = format!(
                                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
                                body.len()
                            )
                            .into_bytes();
                            a.extend_from_slice(body);
                            a.extend_from_slice(b"\r\n0\r\n\r\n");
                            a
                        }
                        Some((_, body, false)) => {
                            let mut a = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            a.extend_from_slice(body);
                            a
                        }
                        None => b"HTTP/1.1 404 Not Found\r\n\r\n".to_vec(),
                    };
                    let _ = sock.write_all(&answer).await;
                });
            }
        });
        base
    }

    #[tokio::test]
    async fn get_reads_plain_and_chunked_answers() {
        let base =
            server(vec![("/a", b"plain".to_vec(), false), ("/b", b"chunks".to_vec(), true)]).await;
        assert_eq!(get(&format!("{base}/a")).await.unwrap(), b"plain");
        assert_eq!(get(&format!("{base}/b")).await.unwrap(), b"chunks");
        assert!(get(&format!("{base}/c")).await.unwrap_err().to_string().contains("404"));
        assert!(get("https://x/a").await.is_err());
        let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = silent.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = silent.accept().await.unwrap();
            let _ = sock.read(&mut [0u8; 1024]).await;
            sock.write_all(b"garbage").await.unwrap();
        });
        assert!(get(&format!("http://{at}")).await.unwrap_err().to_string().contains("no HTTP"));

        // A server that keeps the connection open after the answer.
        let open = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = open.local_addr().unwrap();
        let held = tokio::spawn(async move {
            let (mut sock, _) = open.accept().await.unwrap();
            let _ = sock.read(&mut [0u8; 1024]).await;
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody").await.unwrap();
            tokio::time::sleep(FETCH_TIMEOUT * 2).await;
        });
        let started = Instant::now();
        assert_eq!(get(&format!("http://{at}/x")).await.unwrap(), b"body");
        assert!(started.elapsed() < FETCH_TIMEOUT);
        held.abort();

        let short = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = short.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = short.accept().await.unwrap();
            let _ = sock.read(&mut [0u8; 1024]).await;
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort").await.unwrap();
        });
        assert!(
            get(&format!("http://{at}/x")).await.unwrap_err().to_string().contains("cut short")
        );
    }

    /// Answers every request on a connection with its path, counting connections and requests.
    async fn keep_alive_server() -> (String, Arc<Mutex<(usize, Vec<String>)>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new((0, Vec::new())));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                log.lock().unwrap().0 += 1;
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    while let Ok(n) = sock.read(&mut buf).await {
                        if n == 0 {
                            return;
                        }
                        let ask = String::from_utf8_lossy(&buf[..n]).to_string();
                        let path = ask.split(' ').nth(1).unwrap_or_default().to_string();
                        log.lock().unwrap().1.push(path.clone());
                        let answer = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{path}",
                            path.len()
                        );
                        if sock.write_all(answer.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn one_connection_carries_every_request() {
        let (base, seen) = keep_alive_server().await;
        let mut client = Client::default();
        for path in ["/a", "/b", "/c"] {
            assert_eq!(client.get(&format!("{base}{path}")).await.unwrap(), path.as_bytes());
        }
        assert_eq!(seen.lock().unwrap().0, 1);

        // A server that closes after each answer still serves every request.
        let closing = server(vec![("/a", b"A".to_vec(), false)]).await;
        let mut client = Client::default();
        for _ in 0..3 {
            assert_eq!(client.get(&format!("{closing}/a")).await.unwrap(), b"A");
        }
        let other = server(vec![("/b", b"B".to_vec(), true)]).await;
        assert_eq!(client.get(&format!("{other}/b")).await.unwrap(), b"B");
    }

    #[tokio::test]
    async fn a_server_that_can_hold_the_playlist_is_asked_for_the_next_segment() {
        let (base, seen) = keep_alive_server().await;
        let mut stream = Stream {
            url: format!("{base}/index.m3u8"),
            track: track(),
            next: 7,
            give_up: GIVE_UP,
            client: Client::default(),
            init: "init.mp4".into(),
            discontinuity: 0,
            block_reload: true,
        };
        // The echo server answers with the path, so the playlist read back is empty.
        stream.playlist().await.unwrap();
        assert_eq!(seen.lock().unwrap().1, ["/index.m3u8?_HLS_msn=7"]);
        assert!(!stream.block_reload, "an answer without the flag turns holding off");
        stream.playlist().await.unwrap();
        assert_eq!(seen.lock().unwrap().1.last().unwrap(), "/index.m3u8");
    }

    #[tokio::test]
    async fn a_stream_that_starts_over_is_reported() {
        let playlist = |map: &str, seq: u64, disc: u64| {
            format!(
                "#EXT-X-MEDIA-SEQUENCE:{seq}\n#EXT-X-DISCONTINUITY-SEQUENCE:{disc}\n\
                 #EXT-X-MAP:URI=\"{map}\"\n#EXTINF:1,\nseg.m4s\n"
            )
            .into_bytes()
        };
        let base = server(vec![
            ("/same.m3u8", playlist("init.mp4", 5, 0), false),
            ("/new_init.m3u8", playlist("init_1.mp4", 5, 0), false),
            ("/new_disc.m3u8", playlist("init.mp4", 5, 1), false),
            ("/init.mp4", init(), false),
            ("/seg.m4s", media_segment(&[b"x"]), false),
        ])
        .await;
        let mut stream = Stream::open(&format!("{base}/same.m3u8")).await.unwrap();
        assert!(stream.fetch_next().await.unwrap().is_some());
        for gone in ["new_init", "new_disc"] {
            stream.url = format!("{base}/{gone}.m3u8");
            assert!(started_over(&stream.fetch_next().await.unwrap_err()), "{gone}");
        }
        stream.url = format!("{base}/same.m3u8");
        stream.next = 50;
        let e = stream.fetch_next().await.unwrap_err();
        assert!(started_over(&e) && e.to_string().contains("started over"));
        assert!(!started_over(&io::Error::other("other")));

        stream.next = 5;
        let (tx, _rx) = mpsc::channel(1);
        stream.url = format!("{base}/new_init.m3u8");
        assert!(started_over(&stream.run(tx).await.unwrap_err()));
    }

    fn media_segment(samples: &[&[u8]]) -> Vec<u8> {
        let mut entries = Vec::new();
        for s in samples {
            entries.extend_from_slice(&[s.len() as u32]);
        }
        let probe =
            moof(2, 0x020008, &[1800], 0x201, &[&[samples.len() as u32, 0][..], &entries].concat());
        let offset = probe.len() as u32 + 8;
        let moof = moof(
            2,
            0x020008,
            &[1800],
            0x201,
            &[&[samples.len() as u32, offset][..], &entries].concat(),
        );
        [moof, mp4_box(b"mdat", &samples.concat())].concat()
    }

    #[tokio::test]
    async fn a_live_stream_plays_from_behind_the_newest_segment() {
        let playlist = b"#EXT-X-MEDIA-SEQUENCE:5\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1,\nseg_5.m4s\n#EXTINF:1,\nseg_6.m4s\n".to_vec();
        let base = server(vec![
            ("/live/index.m3u8", playlist, false),
            ("/live/init.mp4", init(), false),
            ("/live/seg_5.m4s", media_segment(&[b"old"]), false),
            ("/live/seg_6.m4s", media_segment(&[b"one", b"two"]), true),
        ])
        .await;
        let stream = Stream::open(&format!("{base}/live/index.m3u8")).await.unwrap();
        assert_eq!(stream.track.width, 720);
        let (tx, mut rx) = mpsc::channel(1);
        let run = tokio::spawn(stream.run(tx));
        let started = Instant::now();
        assert_eq!(rx.recv().await.unwrap(), b"old");
        assert_eq!(rx.recv().await.unwrap(), b"one");
        assert_eq!(rx.recv().await.unwrap(), b"two");
        assert!(started.elapsed() >= Duration::from_millis(40), "paced by the sample duration");
        // Nothing new: the playlist is polled until the receiver goes away.
        tokio::time::sleep(POLL * 3).await;
        drop(rx);
        run.abort();

        let no_map = server(vec![("/p.m3u8", b"#EXTM3U\nseg.m4s\n".to_vec(), false)]).await;
        assert!(Stream::open(&format!("{no_map}/p.m3u8")).await.is_err());
        let bad_init = server(vec![
            ("/p.m3u8", b"#EXT-X-MAP:URI=\"i.mp4\"\n".to_vec(), false),
            ("/i.mp4", b"nothing".to_vec(), false),
        ])
        .await;
        assert!(Stream::open(&format!("{bad_init}/p.m3u8")).await.is_err());
    }

    #[tokio::test]
    async fn the_run_ends_when_nobody_takes_frames_or_the_server_fails() {
        let playlist = b"#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-MAP:URI=\"init.mp4\"\nseg.m4s\n".to_vec();
        let base = server(vec![
            ("/index.m3u8", playlist.clone(), false),
            ("/init.mp4", init(), false),
            ("/seg.m4s", media_segment(&[b"a"]), false),
        ])
        .await;
        let stream = Stream::open(&format!("{base}/index.m3u8")).await.unwrap();
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        stream.run(tx).await.unwrap();

        let gone =
            server(vec![("/index.m3u8", playlist, false), ("/init.mp4", init(), false)]).await;
        let mut stream = Stream::open(&format!("{gone}/index.m3u8")).await.unwrap();
        stream.next = 0;
        stream.give_up = POLL * 2;
        let (tx, _rx) = mpsc::channel(1);
        assert!(stream.run(tx).await.unwrap_err().to_string().contains("404"));
    }
}
