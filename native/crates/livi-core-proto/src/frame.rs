// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

const LEN_BYTES: usize = 4;

pub const MAX_FRAME: usize = 16 << 20;

#[derive(Debug)]
pub enum FrameError {
    TooLarge(usize),
    Json(serde_json::Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(len) => write!(f, "frame of {len} bytes exceeds {MAX_FRAME}"),
            Self::Json(e) => write!(f, "frame is not a valid message: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let mut out = vec![0; LEN_BYTES];
    serde_json::to_writer(&mut out, msg).map_err(FrameError::Json)?;
    let len = out.len() - LEN_BYTES;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    out[..LEN_BYTES].copy_from_slice(&(len as u32).to_le_bytes());
    Ok(out)
}

#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    pub fn next_message<T: DeserializeOwned>(&mut self) -> Option<Result<T, FrameError>> {
        let mut head = [0; LEN_BYTES];
        head.copy_from_slice(self.buf.get(..LEN_BYTES)?);
        let len = u32::from_le_bytes(head) as usize;
        if len > MAX_FRAME {
            return Some(Err(FrameError::TooLarge(len)));
        }
        let body = self.buf.get(LEN_BYTES..LEN_BYTES + len)?;
        let msg = serde_json::from_slice(body).map_err(FrameError::Json);
        self.buf.drain(..LEN_BYTES + len);
        Some(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToCore;

    #[test]
    fn round_trip_split_across_chunks() {
        let a = encode(&ToCore::Resync).unwrap();
        let b = encode(&ToCore::Path {
            path: "/media".into(),
        })
        .unwrap();
        let mut stream = a.clone();
        stream.extend_from_slice(&b);

        let mut dec = Decoder::new();
        for byte in &stream[..a.len() + 3] {
            dec.push(std::slice::from_ref(byte));
        }
        assert_eq!(
            dec.next_message::<ToCore>().unwrap().unwrap(),
            ToCore::Resync
        );
        assert!(dec.next_message::<ToCore>().is_none());

        dec.push(&stream[a.len() + 3..]);
        assert_eq!(
            dec.next_message::<ToCore>().unwrap().unwrap(),
            ToCore::Path {
                path: "/media".into()
            }
        );
        assert!(dec.next_message::<ToCore>().is_none());
    }

    #[test]
    fn length_is_little_endian() {
        let frame = encode(&ToCore::Resync).unwrap();
        let body = br#"{"type":"resync"}"#;
        assert_eq!(&frame[..LEN_BYTES], &(body.len() as u32).to_le_bytes());
        assert_eq!(&frame[LEN_BYTES..], body);
    }

    #[test]
    fn oversized_frame_is_refused_before_it_arrives() {
        let mut dec = Decoder::new();
        dec.push(&((MAX_FRAME + 1) as u32).to_le_bytes());
        assert!(matches!(
            dec.next_message::<ToCore>(),
            Some(Err(FrameError::TooLarge(len))) if len == MAX_FRAME + 1
        ));
    }

    #[test]
    fn invalid_frame_is_skipped() {
        let mut dec = Decoder::new();
        let junk = b"{nope";
        dec.push(&(junk.len() as u32).to_le_bytes());
        dec.push(junk);
        dec.push(&encode(&ToCore::Resync).unwrap());
        assert!(matches!(
            dec.next_message::<ToCore>(),
            Some(Err(FrameError::Json(_)))
        ));
        assert_eq!(
            dec.next_message::<ToCore>().unwrap().unwrap(),
            ToCore::Resync
        );
    }
}
