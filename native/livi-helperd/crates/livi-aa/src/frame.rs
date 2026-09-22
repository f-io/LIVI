// AA frame layout: `[ch][flags][len:u16 BE]`, plus `[total:u32 BE]` on the first
// fragment of a multi-frame message, then the payload. Before TLS the payload is
// `[msgId:u16 BE][proto]`. After it every encrypted payload is one TLS record.

use std::collections::HashMap;

use crate::consts::{FLAG_FIRST, FLAG_LAST};

const HEADER_SHORT: usize = 4;
const HEADER_EXTENDED: usize = 8;
const TLS_RECORD_HEADER: usize = 5;

/// One frame as it came off the wire, payload still including the message id.
#[derive(Debug, PartialEq, Eq)]
pub struct RawFrame {
    pub ch: u8,
    pub flags: u8,
    pub payload: Vec<u8>,
}

impl RawFrame {
    /// Splits a plaintext payload into message id and body.
    pub fn message(&self) -> Option<(u16, &[u8])> {
        split_message(&self.payload)
    }
}

pub fn split_message(payload: &[u8]) -> Option<(u16, &[u8])> {
    if payload.len() < 2 {
        return None;
    }
    Some((u16::from_be_bytes([payload[0], payload[1]]), &payload[2..]))
}

/// A single-frame message: short header, message id, body.
pub fn encode(ch: u8, flags: u8, msg_id: u16, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.extend_from_slice(&msg_id.to_be_bytes());
    payload.extend_from_slice(data);
    encode_raw(ch, flags, &payload)
}

/// A single frame around a ready payload (handshake bytes or a TLS record).
pub fn encode_raw(ch: u8, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SHORT + payload.len());
    out.push(ch);
    out.push(flags);
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// One frame per TLS record. Several records become a FIRST/MIDDLE/LAST run,
/// the first carrying the extended header with the total size.
pub fn encode_records(ch: u8, flags: u8, tls: &[u8]) -> Vec<u8> {
    let records = split_records(tls);
    if records.len() <= 1 {
        return encode_raw(ch, flags, tls);
    }
    let total = tls.len() as u32;
    let mut out = Vec::with_capacity(tls.len() + records.len() * HEADER_EXTENDED);
    let last_index = records.len() - 1;
    for (i, rec) in records.iter().enumerate() {
        let f = match i {
            0 => (flags & !FLAG_LAST) | FLAG_FIRST,
            n if n == last_index => (flags & !FLAG_FIRST) | FLAG_LAST,
            _ => flags & !(FLAG_FIRST | FLAG_LAST),
        };
        out.push(ch);
        out.push(f);
        out.extend_from_slice(&(rec.len() as u16).to_be_bytes());
        if i == 0 {
            out.extend_from_slice(&total.to_be_bytes());
        }
        out.extend_from_slice(rec);
    }
    out
}

/// Splits a byte run into TLS records by their 5-byte headers. Anything that
/// does not parse comes back as one piece.
fn split_records(tls: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + TLS_RECORD_HEADER <= tls.len() {
        let len = u16::from_be_bytes([tls[i + 3], tls[i + 4]]) as usize;
        let end = i + TLS_RECORD_HEADER + len;
        if end > tls.len() {
            return vec![tls];
        }
        out.push(&tls[i..end]);
        i = end;
    }
    if i != tls.len() {
        return vec![tls];
    }
    out
}

struct Header {
    ch: u8,
    flags: u8,
    len: usize,
    header_len: usize,
    total: u32,
}

/// The header at the front of `buf`, or None while it is still incomplete.
fn parse_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_SHORT {
        return None;
    }
    let flags = buf[1];
    let extended = flags & FLAG_FIRST != 0 && flags & FLAG_LAST == 0;
    let header_len = if extended { HEADER_EXTENDED } else { HEADER_SHORT };
    if buf.len() < header_len {
        return None;
    }
    let len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let total = if extended { u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) } else { 0 };
    Some(Header { ch: buf[0], flags, len, header_len, total })
}

/// Splits the byte stream into frames, one per wire frame, fragments included.
#[derive(Default)]
pub struct FrameSplitter {
    buf: Vec<u8>,
}

impl FrameSplitter {
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    pub fn next_frame(&mut self) -> Option<RawFrame> {
        let h = parse_header(&self.buf)?;
        let end = h.header_len + h.len;
        if self.buf.len() < end {
            return None;
        }
        let payload = self.buf[h.header_len..end].to_vec();
        self.buf.drain(..end);
        Some(RawFrame { ch: h.ch, flags: h.flags, payload })
    }
}

/// Splits the byte stream into whole messages, reassembling FIRST/MIDDLE/LAST
/// runs per channel. Used before TLS is up.
#[derive(Default)]
pub struct FrameParser {
    splitter: FrameSplitter,
    fragments: HashMap<u8, (Vec<u8>, u32)>,
}

impl FrameParser {
    pub fn push(&mut self, chunk: &[u8]) {
        self.splitter.push(chunk);
    }

    /// The splitter with whatever is still buffered, for after the handshake.
    pub fn into_splitter(self) -> FrameSplitter {
        self.splitter
    }

    pub fn next_frame(&mut self) -> Option<RawFrame> {
        loop {
            let total = parse_header(&self.splitter.buf).map(|h| h.total).unwrap_or(0);
            let f = self.splitter.next_frame()?;
            let first = f.flags & FLAG_FIRST != 0;
            let last = f.flags & FLAG_LAST != 0;
            if first && last {
                return Some(f);
            }
            if first {
                self.fragments.insert(f.ch, (f.payload, total));
                continue;
            }
            let Some((mut buf, total)) = self.fragments.remove(&f.ch) else {
                continue;
            };
            buf.extend_from_slice(&f.payload);
            if !last {
                self.fragments.insert(f.ch, (buf, total));
                continue;
            }
            if total != 0 && buf.len() != total as usize {
                eprintln!("[aa] ch={} reassembled {}B, announced {total}B", f.ch, buf.len());
            }
            return Some(RawFrame { ch: f.ch, flags: f.flags, payload: buf });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{FLAGS_ENC_SIGNAL, FLAGS_PLAINTEXT};

    #[test]
    fn a_message_round_trips() {
        let wire = encode(0, FLAGS_PLAINTEXT, 0x0001, &[0, 1, 0, 7]);
        assert_eq!(&wire[..4], &[0, 3, 0, 6]);
        let mut p = FrameParser::default();
        p.push(&wire);
        let f = p.next_frame().unwrap();
        assert_eq!(f.message(), Some((1u16, &[0u8, 1, 0, 7][..])));
        assert_eq!(p.next_frame(), None);
    }

    #[test]
    fn a_frame_split_across_reads_waits() {
        let wire = encode(3, FLAGS_ENC_SIGNAL, 0x8004, &[8, 1, 16, 1]);
        let mut s = FrameSplitter::default();
        s.push(&wire[..3]);
        assert_eq!(s.next_frame(), None);
        s.push(&wire[3..]);
        assert_eq!(s.next_frame().unwrap().payload.len(), 6);
    }

    #[test]
    fn fragments_are_reassembled_by_channel() {
        // FIRST carries the extended header with the total size.
        let mut wire = vec![5, 0x09, 0, 2, 0, 0, 0, 5, 0xAA, 0xBB];
        wire.extend_from_slice(&[5, 0x08, 0, 1, 0xCC]);
        wire.extend_from_slice(&[5, 0x0a, 0, 2, 0xDD, 0xEE]);
        let mut p = FrameParser::default();
        p.push(&wire);
        let f = p.next_frame().unwrap();
        assert_eq!(f.ch, 5);
        assert_eq!(f.payload, vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    }

    #[test]
    fn the_splitter_keeps_fragments_apart() {
        let mut wire = vec![5, 0x09, 0, 2, 0, 0, 0, 3, 0xAA, 0xBB];
        wire.extend_from_slice(&[5, 0x0a, 0, 1, 0xCC]);
        let mut s = FrameSplitter::default();
        s.push(&wire);
        assert_eq!(s.next_frame().unwrap().payload, vec![0xAA, 0xBB]);
        assert_eq!(s.next_frame().unwrap().payload, vec![0xCC]);
    }

    #[test]
    fn a_continuation_without_a_first_is_dropped() {
        let mut p = FrameParser::default();
        p.push(&[5, 0x0a, 0, 1, 0xCC]);
        assert_eq!(p.next_frame(), None);
    }

    fn record(body: &[u8]) -> Vec<u8> {
        let mut r = vec![0x17, 0x03, 0x03, 0, body.len() as u8];
        r.extend_from_slice(body);
        r
    }

    #[test]
    fn one_record_is_one_frame() {
        let rec = record(&[1, 2, 3]);
        let wire = encode_records(3, FLAGS_ENC_SIGNAL, &rec);
        assert_eq!(wire, encode_raw(3, FLAGS_ENC_SIGNAL, &rec));
    }

    #[test]
    fn several_records_become_a_fragment_run() {
        let mut tls = record(&[1, 2]);
        tls.extend(record(&[3]));
        tls.extend(record(&[4, 5, 6]));
        let wire = encode_records(3, FLAGS_ENC_SIGNAL, &tls);

        let mut s = FrameSplitter::default();
        s.push(&wire);
        let a = s.next_frame().unwrap();
        let b = s.next_frame().unwrap();
        let c = s.next_frame().unwrap();
        assert_eq!(a.flags, 0x09);
        assert_eq!(b.flags, 0x08);
        assert_eq!(c.flags, 0x0a);
        assert_eq!([a.payload, b.payload, c.payload].concat(), tls);

        let mut p = FrameParser::default();
        p.push(&wire);
        assert_eq!(p.next_frame().unwrap().payload, tls);
    }

    #[test]
    fn unparsable_records_stay_one_frame() {
        let wire = encode_records(3, FLAGS_ENC_SIGNAL, &[0x17, 0x03]);
        assert_eq!(wire, encode_raw(3, FLAGS_ENC_SIGNAL, &[0x17, 0x03]));
    }
}
