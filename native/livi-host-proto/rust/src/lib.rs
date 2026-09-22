//! Framing of the gst-host control protocol.
//!
//! The main process and the host exchange length-prefixed messages: four bytes
//! of payload length in host order, then one byte of opcode, four bytes of id,
//! and the rest. A payload shorter than those five bytes carries nothing and is
//! skipped.

pub mod feed;

const LEN_BYTES: usize = 4;
const HEAD_BYTES: usize = 5;

/// One complete message.
#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    pub op: u8,
    pub id: u32,
    pub rest: Vec<u8>,
}

/// Collects bytes until whole messages come out.
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

    /// The next complete message, or None while one is still arriving.
    pub fn next_message(&mut self) -> Option<Message> {
        loop {
            if self.buf.len() < LEN_BYTES {
                return None;
            }
            let len = u32::from_ne_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]])
                as usize;
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

/// The wire form of a reply.
pub fn encode_reply(op: u8, id: u32, rest: &[u8]) -> Vec<u8> {
    let len = (HEAD_BYTES + rest.len()) as u32;
    let mut out = Vec::with_capacity(LEN_BYTES + len as usize);
    out.extend_from_slice(&len.to_ne_bytes());
    out.push(op);
    out.extend_from_slice(&id.to_ne_bytes());
    out.extend_from_slice(rest);
    out
}

/// The plane the main stream is drawn on.
pub const PLANE_MAIN: u32 = 0x7a00_0001;
/// The receiver id whose frames every cluster plane shares.
pub const CLUSTER_RECV_ID: u32 = 0x7a00_0010;
pub const CLUSTER_PLANE_MIN: u32 = 0x7a00_0011;
pub const CLUSTER_PLANE_MAX: u32 = 0x7a00_0013;

pub fn is_cluster_plane(id: u32) -> bool {
    (CLUSTER_PLANE_MIN..=CLUSTER_PLANE_MAX).contains(&id)
}

/// The stream a plane is fed from: cluster planes share one, any other has its own.
pub fn feeder_of(plane_id: u32) -> u32 {
    if is_cluster_plane(plane_id) { CLUSTER_RECV_ID } else { plane_id }
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

        assert_eq!(
            f.next_message(),
            Some(Message { op: 2, id: 0x7a00_0001, rest: vec![1, 2, 3] })
        );
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
    fn a_reply_carries_its_length_ahead_of_the_header() {
        let out = encode_reply(2, 0x0102_0304, &[7, 8]);

        assert_eq!(out.len(), 4 + 5 + 2);
        assert_eq!(u32::from_ne_bytes([out[0], out[1], out[2], out[3]]), 7);
        assert_eq!(out[4], 2);
        assert_eq!(u32::from_ne_bytes([out[5], out[6], out[7], out[8]]), 0x0102_0304);
        assert_eq!(&out[9..], &[7, 8]);
    }
}
