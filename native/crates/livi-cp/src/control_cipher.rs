// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::crypto::{chacha_open, chacha_seal, nonce64};

const MAX_PAYLOAD: usize = 0x4000;

#[derive(Debug, PartialEq, Eq)]
pub struct AuthFailed;

pub struct ControlCipher {
    read_key: [u8; 32],
    write_key: [u8; 32],
    read_ctr: u64,
    write_ctr: u64,
}

impl ControlCipher {
    pub fn new(read_key: [u8; 32], write_key: [u8; 32]) -> Self {
        Self {
            read_key,
            write_key,
            read_ctr: 0,
            write_ctr: 0,
        }
    }

    pub fn decrypt(&mut self, buf: &[u8]) -> Result<(Vec<u8>, Vec<u8>), AuthFailed> {
        let mut out = Vec::new();
        let mut off = 0;
        while buf.len() - off >= 2 {
            let len = usize::from(u16::from_le_bytes([buf[off], buf[off + 1]]));
            let end = off + 2 + len + 16;
            if buf.len() < end {
                break;
            }
            let plain = chacha_open(
                &self.read_key,
                &nonce64(self.read_ctr),
                &buf[off + 2..end],
                &buf[off..off + 2],
            )
            .ok_or(AuthFailed)?;
            out.extend_from_slice(&plain);
            self.read_ctr += 1;
            off = end;
        }
        Ok((out, buf[off..].to_vec()))
    }

    pub fn encrypt(&mut self, plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let chunk = &plain[i..(i + MAX_PAYLOAD).min(plain.len())];
            let header = (chunk.len() as u16).to_le_bytes();
            out.extend_from_slice(&header);
            out.extend(chacha_seal(
                &self.write_key,
                &nonce64(self.write_ctr),
                chunk,
                &header,
            ));
            self.write_ctr += 1;
            i += MAX_PAYLOAD;
            if i >= plain.len() {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (ControlCipher, ControlCipher) {
        (
            ControlCipher::new([1; 32], [2; 32]),
            ControlCipher::new([2; 32], [1; 32]),
        )
    }

    #[test]
    fn frames_round_trip_across_partial_reads() {
        let (mut ours, mut theirs) = pair();
        let a = theirs.encrypt(b"GET /info RTSP/1.0\r\n\r\n");
        let b = theirs.encrypt(b"OPTIONS * RTSP/1.0\r\n\r\n");
        let stream = [a.clone(), b].concat();

        let (plain, rest) = ours.decrypt(&stream[..a.len() + 5]).unwrap();
        assert_eq!(plain, b"GET /info RTSP/1.0\r\n\r\n");
        assert_eq!(rest.len(), 5);
        let (plain, rest) = ours
            .decrypt(&[rest, stream[a.len() + 5..].to_vec()].concat())
            .unwrap();
        assert_eq!(plain, b"OPTIONS * RTSP/1.0\r\n\r\n");
        assert!(rest.is_empty());
    }

    #[test]
    fn large_messages_split_at_16k() {
        let (mut ours, mut theirs) = pair();
        let big = vec![0x5a; 0x4000 + 10];
        let sealed = theirs.encrypt(&big);
        assert_eq!(sealed.len(), 2 + 0x4000 + 16 + 2 + 10 + 16);
        assert_eq!(ours.decrypt(&sealed).unwrap().0, big);
        assert_eq!(theirs.encrypt(&[]).len(), 2 + 16);
    }

    #[test]
    fn a_tampered_frame_fails() {
        let (mut ours, mut theirs) = pair();
        let mut sealed = theirs.encrypt(b"hello");
        sealed[3] ^= 1;
        assert_eq!(ours.decrypt(&sealed), Err(AuthFailed));
    }
}
