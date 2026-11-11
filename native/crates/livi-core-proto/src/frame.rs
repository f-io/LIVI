// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;

use livi_base::frame::{self, Frames, LEN_BYTES, TooLarge};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use livi_base::frame::MAX_FRAME;

#[derive(Debug)]
pub enum FrameError {
    TooLarge(usize),
    Json(serde_json::Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(len) => TooLarge(*len).fmt(f),
            Self::Json(e) => write!(f, "frame is not a valid message: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let mut out = frame::open(0);
    serde_json::to_writer(&mut out, msg).map_err(FrameError::Json)?;
    let len = out.len() - LEN_BYTES;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    Ok(frame::seal(out))
}

#[derive(Default)]
pub struct Decoder {
    frames: Frames,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.frames.push(chunk);
    }

    pub fn next_message<T: DeserializeOwned>(&mut self) -> Option<Result<T, FrameError>> {
        let msg = self
            .frames
            .next_with(|body| serde_json::from_slice::<T>(body))?;
        Some(match msg {
            Ok(msg) => msg.map_err(FrameError::Json),
            Err(TooLarge(len)) => Err(FrameError::TooLarge(len)),
        })
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
