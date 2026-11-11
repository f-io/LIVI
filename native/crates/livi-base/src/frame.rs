// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;

pub const LEN_BYTES: usize = 4;

pub const MAX_FRAME: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge(pub usize);

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame of {} bytes exceeds {MAX_FRAME}", self.0)
    }
}

impl std::error::Error for TooLarge {}

pub fn open(body_capacity: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(LEN_BYTES + body_capacity);
    out.extend_from_slice(&[0; LEN_BYTES]);
    out
}

pub fn seal(mut out: Vec<u8>) -> Vec<u8> {
    let len = (out.len() - LEN_BYTES) as u32;
    out[..LEN_BYTES].copy_from_slice(&len.to_le_bytes());
    out
}

#[derive(Default)]
pub struct Frames {
    buf: Vec<u8>,
    broken: Option<TooLarge>,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        if self.broken.is_none() {
            self.buf.extend_from_slice(chunk);
        }
    }

    pub fn next_with<R>(&mut self, read: impl FnOnce(&[u8]) -> R) -> Option<Result<R, TooLarge>> {
        if let Some(e) = self.broken {
            return Some(Err(e));
        }
        let head: [u8; LEN_BYTES] = *self.buf.first_chunk()?;
        let len = u32::from_le_bytes(head) as usize;
        if len > MAX_FRAME {
            self.buf = Vec::new();
            self.broken = Some(TooLarge(len));
            return self.broken.map(Err);
        }
        let out = read(self.buf.get(LEN_BYTES..LEN_BYTES + len)?);
        self.buf.drain(..LEN_BYTES + len);
        Some(Ok(out))
    }

    pub fn next_frame(&mut self) -> Option<Result<Vec<u8>, TooLarge>> {
        self.next_with(<[u8]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut out = open(body.len());
        out.extend_from_slice(body);
        seal(out)
    }

    #[test]
    fn the_length_is_little_endian_and_excludes_itself() {
        assert_eq!(frame(b"abc"), [3, 0, 0, 0, b'a', b'b', b'c']);
        assert_eq!(frame(&[]), [0; LEN_BYTES]);
    }

    #[test]
    fn frames_split_across_chunks_come_out_whole_and_in_order() {
        let mut wire = frame(b"first");
        wire.extend(frame(b""));
        wire.extend(frame(b"third"));
        let mut f = Frames::new();
        for byte in &wire {
            f.push(std::slice::from_ref(byte));
        }
        assert_eq!(f.next_frame(), Some(Ok(b"first".to_vec())));
        assert_eq!(f.next_frame(), Some(Ok(Vec::new())));
        assert_eq!(f.next_frame(), Some(Ok(b"third".to_vec())));
        assert_eq!(f.next_frame(), None);
    }

    #[test]
    fn a_partial_frame_waits_for_the_rest() {
        let wire = frame(b"payload");
        let mut f = Frames::new();
        f.push(&wire[..2]);
        assert_eq!(f.next_frame(), None);
        f.push(&wire[2..6]);
        assert_eq!(f.next_frame(), None);
        f.push(&wire[6..]);
        assert_eq!(f.next_frame(), Some(Ok(b"payload".to_vec())));
    }

    #[test]
    fn an_oversized_length_breaks_the_stream_for_good() {
        let mut f = Frames::new();
        f.push(&((MAX_FRAME + 1) as u32).to_le_bytes());
        let broken = Some(Err(TooLarge(MAX_FRAME + 1)));
        assert_eq!(f.next_frame(), broken);
        f.push(&frame(b"later"));
        assert_eq!(f.next_frame(), broken);
        assert!(f.buf.is_empty());
    }

    #[test]
    fn the_largest_allowed_length_is_still_awaited() {
        let mut f = Frames::new();
        f.push(&(MAX_FRAME as u32).to_le_bytes());
        assert_eq!(f.next_frame(), None);
    }
}
