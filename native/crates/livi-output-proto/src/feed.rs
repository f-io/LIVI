// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_base::frame::{self, Frames, TooLarge};

const HEAD_BYTES: usize = 13;

pub const KIND_VIDEO: u8 = 1;
pub const KIND_AUDIO: u8 = 2;
pub const KIND_VIDEO_START: u8 = 3;
pub const KIND_MIC: u8 = 4;

#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub id: u32,
    pub ts: u64,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct Framer {
    frames: Frames,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.frames.push(chunk);
    }

    pub fn next_record(&mut self) -> Option<Result<Record, TooLarge>> {
        loop {
            let record = self.frames.next_with(|body| {
                let (head, payload) = body.split_first_chunk::<HEAD_BYTES>()?;
                Some(Record {
                    kind: head[0],
                    id: u32::from_le_bytes(head[1..5].try_into().ok()?),
                    ts: u64::from_le_bytes(head[5..].try_into().ok()?),
                    payload: payload.to_vec(),
                })
            })?;
            if let Some(record) = record.transpose() {
                return Some(record);
            }
        }
    }
}

pub fn encode(kind: u8, id: u32, ts: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = frame::open(HEAD_BYTES + payload.len());
    out.push(kind);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&ts.to_le_bytes());
    out.extend_from_slice(payload);
    frame::seal(out)
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
            Some(Ok(Record {
                kind: KIND_VIDEO,
                id: 0x7a00_0001,
                ts: 42,
                payload: vec![1, 2, 3]
            }))
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
        assert_eq!(f.next_record().unwrap().unwrap().payload.len(), 40);
    }

    #[test]
    fn several_records_in_one_chunk_come_out_in_order() {
        let mut buf = encode(KIND_VIDEO_START, 10, 0, &[0]);
        buf.extend(encode(KIND_VIDEO, 10, 1, b"nal"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_record().unwrap().unwrap().kind, KIND_VIDEO_START);
        assert_eq!(f.next_record().unwrap().unwrap().payload, b"nal");
        assert_eq!(f.next_record(), None);
    }

    #[test]
    fn a_payload_too_short_for_a_header_is_skipped() {
        let mut buf = 3u32.to_le_bytes().to_vec();
        buf.extend_from_slice(&[9, 9, 9]);
        buf.extend(encode(KIND_VIDEO, 5, 0, b"ok"));
        let mut f = Framer::new();
        f.push(&buf);

        assert_eq!(f.next_record().unwrap().unwrap().id, 5);
    }

    #[test]
    fn an_empty_payload_is_still_a_record() {
        let mut f = Framer::new();
        f.push(&encode(KIND_VIDEO, 99, 5, &[]));
        assert_eq!(
            f.next_record(),
            Some(Ok(Record {
                kind: KIND_VIDEO,
                id: 99,
                ts: 5,
                payload: vec![]
            }))
        );
    }
}
