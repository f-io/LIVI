//! Framing of the media feed a helper streams into the host: four bytes of
//! payload length in host order, one byte of kind, four bytes of stream id,
//! eight bytes of timestamp, and the payload.

const LEN_BYTES: usize = 4;
const HEAD_BYTES: usize = 13;

/// One video access unit for the plane `id` names.
pub const KIND_VIDEO: u8 = 1;
/// Samples for the audio stream `id` names.
pub const KIND_AUDIO: u8 = 2;
/// Announces the codec of the video stream: payload is one byte, 0 for H.264,
/// 1 for H.265.
pub const KIND_VIDEO_START: u8 = 3;
/// Captured microphone samples, from the pipeline to whoever asked for the tap.
pub const KIND_MIC: u8 = 4;

/// One complete record.
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub id: u32,
    pub ts: u64,
    pub payload: Vec<u8>,
}

/// Collects bytes until whole records come out.
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

    /// The next complete record, or None while one is still arriving.
    pub fn next_record(&mut self) -> Option<Record> {
        loop {
            if self.buf.len() < LEN_BYTES {
                return None;
            }
            let len = u32::from_ne_bytes(self.buf[..LEN_BYTES].try_into().unwrap()) as usize;
            if self.buf.len() < LEN_BYTES + len {
                return None;
            }
            if len < HEAD_BYTES {
                self.buf.drain(..LEN_BYTES + len);
                continue;
            }
            let head = &self.buf[LEN_BYTES..LEN_BYTES + HEAD_BYTES];
            let record = Record {
                kind: head[0],
                id: u32::from_ne_bytes(head[1..5].try_into().unwrap()),
                ts: u64::from_ne_bytes(head[5..13].try_into().unwrap()),
                payload: self.buf[LEN_BYTES + HEAD_BYTES..LEN_BYTES + len].to_vec(),
            };
            self.buf.drain(..LEN_BYTES + len);
            return Some(record);
        }
    }
}

/// The wire form of one record.
pub fn encode(kind: u8, id: u32, ts: u64, payload: &[u8]) -> Vec<u8> {
    let len = (HEAD_BYTES + payload.len()) as u32;
    let mut out = Vec::with_capacity(LEN_BYTES + len as usize);
    out.extend_from_slice(&len.to_ne_bytes());
    out.push(kind);
    out.extend_from_slice(&id.to_ne_bytes());
    out.extend_from_slice(&ts.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_comes_back_out() {
        let mut f = Framer::new();
        f.push(&encode(KIND_VIDEO, 0x7a00_0001, 42, &[1, 2, 3]));

        assert_eq!(
            f.next_record(),
            Some(Record { kind: KIND_VIDEO, id: 0x7a00_0001, ts: 42, payload: vec![1, 2, 3] })
        );
        assert_eq!(f.next_record(), None);
    }

    #[test]
    fn a_record_split_across_chunks_is_reassembled() {
        let msg = encode(KIND_AUDIO, 7, 0, &[9; 40]);
        let mut f = Framer::new();
        for piece in msg.chunks(5) {
            f.push(piece);
        }
        assert_eq!(f.next_record().unwrap().payload.len(), 40);
    }

    #[test]
    fn several_records_in_one_chunk_come_out_in_order() {
        let mut buf = encode(KIND_VIDEO_START, 10, 0, &[0]);
        buf.extend(encode(KIND_VIDEO, 10, 1, b"nal"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_record().unwrap().kind, KIND_VIDEO_START);
        assert_eq!(f.next_record().unwrap().payload, b"nal");
        assert_eq!(f.next_record(), None);
    }

    #[test]
    fn a_payload_too_short_for_a_header_is_skipped() {
        let mut buf = 3u32.to_ne_bytes().to_vec();
        buf.extend_from_slice(&[9, 9, 9]);
        buf.extend(encode(KIND_VIDEO, 5, 0, b"ok"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_record().unwrap().id, 5);
    }

    #[test]
    fn an_empty_payload_is_still_a_record() {
        let mut f = Framer::new();
        f.push(&encode(KIND_VIDEO, 99, 5, &[]));
        assert_eq!(f.next_record(), Some(Record { kind: KIND_VIDEO, id: 99, ts: 5, payload: vec![] }));
    }
}
