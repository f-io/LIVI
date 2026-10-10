//! Framing of the gst-host control protocol: four bytes of payload length in
//! host order, then one byte of opcode, four bytes of id and the rest. A
//! payload shorter than five bytes is skipped.

pub mod feed;
pub mod ui_planes;

const LEN_BYTES: usize = 4;
const HEAD_BYTES: usize = 5;

#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    pub op: u8,
    pub id: u32,
    pub rest: Vec<u8>,
}

#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    pub fn next_message(&mut self) -> Option<Message> {
        loop {
            if self.buf.len() < LEN_BYTES {
                return None;
            }
            let len =
                u32::from_ne_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if self.buf.len() < LEN_BYTES + len {
                return None;
            }

            let frame: Vec<u8> = self.buf.drain(..LEN_BYTES + len).skip(LEN_BYTES).collect();
            if len < HEAD_BYTES {
                continue;
            }
            return Some(Message {
                op: frame[0],
                id: u32::from_ne_bytes([frame[1], frame[2], frame[3], frame[4]]),
                rest: frame[HEAD_BYTES..].to_vec(),
            });
        }
    }
}

/// Encodes requests as well as replies.
pub fn encode_reply(op: u8, id: u32, rest: &[u8]) -> Vec<u8> {
    let len = (HEAD_BYTES + rest.len()) as u32;
    let mut out = Vec::with_capacity(LEN_BYTES + len as usize);
    out.extend_from_slice(&len.to_ne_bytes());
    out.push(op);
    out.extend_from_slice(&id.to_ne_bytes());
    out.extend_from_slice(rest);
    out
}

pub const PLANE_MAIN: u32 = 0x7a00_0001;
pub const PLANE_VIDEO: u32 = 0x7a00_0002;
pub const PLANE_CLUSTER_RECV: u32 = 0x7a00_0010;
pub const CLUSTER_RECV_ID: u32 = 0x7a00_0010;
pub const CLUSTER_PLANE_MIN: u32 = 0x7a00_0011;
pub const CLUSTER_PLANE_MAX: u32 = 0x7a00_0013;

pub fn is_cluster_plane(id: u32) -> bool {
    (CLUSTER_PLANE_MIN..=CLUSTER_PLANE_MAX).contains(&id)
}

pub fn feeder_of(plane_id: u32) -> u32 {
    if is_cluster_plane(plane_id) { CLUSTER_RECV_ID } else { plane_id }
}

/// The cluster plane ids follow this order.
pub const SCREENS: [&str; 3] = ["main", "dash", "aux"];
pub const MAIN_TAG: &str = "main";
pub const VIDEO_TAG: &str = "video";

pub fn cluster_tag(screen: &str) -> String {
    format!("cluster-{screen}")
}

pub fn plane_place(id: u32) -> Option<(String, &'static str)> {
    if id == PLANE_MAIN {
        return Some((MAIN_TAG.into(), SCREENS[0]));
    }
    if id == PLANE_VIDEO {
        return Some((VIDEO_TAG.into(), SCREENS[0]));
    }
    let screen = SCREENS.get(id.checked_sub(CLUSTER_PLANE_MIN)? as usize)?;
    Some((cluster_tag(screen), screen))
}

const CODEC_MAX: usize = 15;

/// `[1B codecLen][codec ascii][codec_data]`
pub fn plane_body(codec: &str, codec_data: &[u8]) -> Vec<u8> {
    let codec = &codec.as_bytes()[..codec.len().min(CODEC_MAX)];
    let mut rest = Vec::with_capacity(1 + codec.len() + codec_data.len());
    rest.push(codec.len() as u8);
    rest.extend_from_slice(codec);
    rest.extend_from_slice(codec_data);
    rest
}

pub fn parse_plane_body(rest: &[u8]) -> Option<(String, &[u8])> {
    let len = usize::from(*rest.first()?).min(CODEC_MAX);
    let codec = rest.get(1..1 + len)?;
    Some((String::from_utf8_lossy(codec).into_owned(), &rest[1 + len..]))
}

/// `[2B deviceLen LE][audio device][url]`, an empty device for the system default.
pub fn url_body(url: &str, audio_device: &str) -> Vec<u8> {
    let device = &audio_device.as_bytes()[..audio_device.len().min(usize::from(u16::MAX))];
    let mut rest = Vec::with_capacity(2 + device.len() + url.len());
    rest.extend_from_slice(&(device.len() as u16).to_le_bytes());
    rest.extend_from_slice(device);
    rest.extend_from_slice(url.as_bytes());
    rest
}

/// (url, audio device)
pub fn parse_url_body(rest: &[u8]) -> Option<(String, String)> {
    let len = usize::from(u16::from_le_bytes([*rest.first()?, *rest.get(1)?]));
    let device = core::str::from_utf8(rest.get(2..2 + len)?).ok()?;
    let url = core::str::from_utf8(&rest[2 + len..]).ok().filter(|u| !u.is_empty())?;
    Some((url.to_owned(), device.to_owned()))
}

/// What a URL player knows about itself. Times are seconds, None while the player cannot tell,
/// a live stream has no duration.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UrlStatus {
    pub position: Option<f64>,
    pub duration: Option<f64>,
    /// (start, end) of what can be sought to.
    pub seekable: Option<(f64, f64)>,
    pub playing: bool,
    /// Paused or playing, the stream opened.
    pub ready: bool,
    /// Of the last buffering report, None when the player sent none.
    pub buffered_percent: Option<u8>,
    pub ended: bool,
    pub failed: bool,
}

const STATUS_PLAYING: u8 = 1;
const STATUS_READY: u8 = 2;
const STATUS_ENDED: u8 = 4;
const STATUS_FAILED: u8 = 8;
const STATUS_BUFFERING: u8 = 16;

/// `[1B flags][1B buffered percent][8B position][8B duration][8B seek start][8B seek end]`,
/// times as f64 LE, NaN for none.
pub fn url_status_body(s: &UrlStatus) -> Vec<u8> {
    let flag = |on: bool, bit: u8| if on { bit } else { 0 };
    let flags = flag(s.playing, STATUS_PLAYING)
        | flag(s.ready, STATUS_READY)
        | flag(s.ended, STATUS_ENDED)
        | flag(s.failed, STATUS_FAILED)
        | flag(s.buffered_percent.is_some(), STATUS_BUFFERING);
    let mut rest = vec![flags, s.buffered_percent.unwrap_or(0)];
    let (start, end) = s.seekable.unzip();
    for t in [s.position, s.duration, start, end] {
        rest.extend_from_slice(&t.unwrap_or(f64::NAN).to_le_bytes());
    }
    rest
}

pub fn parse_url_status(rest: &[u8]) -> Option<UrlStatus> {
    let (&flags, rest) = rest.split_first()?;
    let (&percent, rest) = rest.split_first()?;
    let (times, _) = rest.as_chunks::<8>();
    if times.len() < 4 {
        return None;
    }
    let time = |i: usize| Some(f64::from_le_bytes(times[i])).filter(|t| !t.is_nan());
    Some(UrlStatus {
        position: time(0),
        duration: time(1),
        seekable: time(2).zip(time(3)),
        playing: flags & STATUS_PLAYING != 0,
        ready: flags & STATUS_READY != 0,
        buffered_percent: (flags & STATUS_BUFFERING != 0).then_some(percent),
        ended: flags & STATUS_ENDED != 0,
        failed: flags & STATUS_FAILED != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(op: u8, id: u32, rest: &[u8]) -> Vec<u8> {
        encode_reply(op, id, rest)
    }

    #[test]
    fn a_whole_message_comes_back_out() {
        let mut f = Framer::new();
        f.push(&frame(2, 0x7a00_0001, &[1, 2, 3]));

        assert_eq!(f.next_message(), Some(Message { op: 2, id: 0x7a00_0001, rest: vec![1, 2, 3] }));
        assert_eq!(f.next_message(), None);
    }

    #[test]
    fn a_message_split_across_chunks_is_reassembled() {
        let msg = frame(3, 7, &[9; 40]);
        let mut f = Framer::new();

        for piece in msg.chunks(6) {
            f.push(piece);
        }

        assert_eq!(f.next_message().unwrap().rest.len(), 40);
    }

    #[test]
    fn several_messages_in_one_chunk_come_out_in_order() {
        let mut buf = frame(1, 10, b"a");
        buf.extend(frame(2, 20, b"bb"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_message().unwrap().id, 10);
        assert_eq!(f.next_message().unwrap().id, 20);
        assert_eq!(f.next_message(), None);
    }

    #[test]
    fn a_payload_too_short_for_a_header_is_skipped() {
        let mut buf = 3u32.to_ne_bytes().to_vec();
        buf.extend_from_slice(&[9, 9, 9]);
        buf.extend(frame(4, 5, b"ok"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_message().unwrap().id, 5);
    }

    #[test]
    fn a_message_with_no_rest_is_still_a_message() {
        let mut f = Framer::new();
        f.push(&frame(3, 99, &[]));

        assert_eq!(f.next_message(), Some(Message { op: 3, id: 99, rest: vec![] }));
    }

    #[test]
    fn nothing_comes_out_of_nothing() {
        let mut f = Framer::new();
        assert_eq!(f.next_message(), None);
        f.push(&[1, 2, 3]);
        assert_eq!(f.next_message(), None);
    }

    #[test]
    fn every_plane_has_its_tag_and_screen() {
        assert_eq!(plane_place(PLANE_MAIN), Some(("main".into(), "main")));
        assert_eq!(plane_place(PLANE_VIDEO), Some(("video".into(), "main")));
        assert_eq!(plane_place(CLUSTER_PLANE_MIN + 1), Some(("cluster-dash".into(), "dash")));
        assert_eq!(plane_place(CLUSTER_PLANE_MAX), Some(("cluster-aux".into(), "aux")));
        assert_eq!(plane_place(PLANE_CLUSTER_RECV), None);
        assert_eq!(plane_place(CLUSTER_PLANE_MAX + 1), None);
    }

    #[test]
    fn a_plane_body_comes_back_apart() {
        let body = plane_body("h265", &[1, 2]);
        assert_eq!(body, [&[4u8][..], b"h265", &[1, 2]].concat());
        assert_eq!(parse_plane_body(&body), Some(("h265".into(), &[1u8, 2][..])));
        assert_eq!(parse_plane_body(&[4, b'h', b'2']), None);
        assert_eq!(parse_plane_body(&[]), None);
    }

    #[test]
    fn a_url_body_comes_back_apart() {
        let body = url_body("http://127.0.0.1:9/a.m3u8", "speakers");
        assert_eq!(&body[..2], &[8, 0]);
        assert_eq!(
            parse_url_body(&body),
            Some(("http://127.0.0.1:9/a.m3u8".into(), "speakers".into()))
        );
        assert_eq!(
            parse_url_body(&url_body("http://x/", "")),
            Some(("http://x/".into(), "".into()))
        );
    }

    #[test]
    fn a_url_body_without_its_url_is_nothing() {
        assert_eq!(parse_url_body(&url_body("", "speakers")), None);
        assert_eq!(parse_url_body(&[9, 0, b's']), None);
        assert_eq!(parse_url_body(&[1]), None);
        assert_eq!(parse_url_body(&[0, 0, 0xff]), None);
    }

    #[test]
    fn a_reply_carries_its_length_ahead_of_the_header() {
        let out = encode_reply(2, 0x0102_0304, &[7, 8]);

        assert_eq!(out.len(), 4 + 5 + 2);
        assert_eq!(u32::from_ne_bytes([out[0], out[1], out[2], out[3]]), 7);
        assert_eq!(out[4], 2);
        assert_eq!(u32::from_ne_bytes([out[5], out[6], out[7], out[8]]), 0x0102_0304);
        assert_eq!(&out[9..], &[7, 8]);
    }

    #[test]
    fn a_url_status_comes_back_apart() {
        let full = UrlStatus {
            position: Some(12.5),
            duration: Some(600.0),
            seekable: Some((0.0, 600.0)),
            playing: true,
            ready: true,
            buffered_percent: Some(40),
            ended: true,
            failed: true,
        };
        assert_eq!(parse_url_status(&url_status_body(&full)), Some(full));
        let live = UrlStatus { position: Some(3.0), ready: true, ..Default::default() };
        assert_eq!(parse_url_status(&url_status_body(&live)), Some(live));
        assert_eq!(parse_url_status(&url_status_body(&live)[..20]), None);
        assert_eq!(parse_url_status(&[1]), None);
        assert_eq!(parse_url_status(&[]), None);
    }
}
