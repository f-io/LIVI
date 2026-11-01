// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::VecDeque;

use crate::fec::crc16_ok;
use crate::rs::{self, Gf};

const FIRE_POLY: u16 = 0x782f;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub dac_48k: bool,
    pub sbr: bool,
    pub ps: bool,
    pub stereo: bool,
}

impl AudioFormat {
    pub fn sample_rate(&self) -> u32 {
        if self.dac_48k { 48_000 } else { 32_000 }
    }

    pub fn core_rate(&self) -> u32 {
        if self.sbr {
            self.sample_rate() / 2
        } else {
            self.sample_rate()
        }
    }

    fn units(&self) -> usize {
        match (self.dac_48k, self.sbr) {
            (false, false) => 4,
            (false, true) => 2,
            (true, false) => 6,
            (true, true) => 3,
        }
    }

    pub fn audio_specific_config(&self) -> Vec<u8> {
        let core_index = sample_rate_index(self.core_rate());
        let channels = if self.stereo { 2 } else { 1 };
        let mut asc = vec![
            (2 << 3) | (core_index >> 1),
            ((core_index & 1) << 7) | (channels << 3) | 0b100,
        ];
        if self.sbr {
            let ext_index = sample_rate_index(self.sample_rate());
            asc.extend_from_slice(&[0x56, 0xe5, 0x80 | (ext_index << 3)]);
            if self.ps {
                asc[4] |= 0x05;
                asc.extend_from_slice(&[0x48, 0x80]);
            }
        }
        asc
    }

    pub fn output_channels(&self) -> u32 {
        if self.stereo || self.ps { 2 } else { 1 }
    }
}

fn sample_rate_index(rate: u32) -> u8 {
    match rate {
        48_000 => 3,
        32_000 => 5,
        24_000 => 6,
        _ => 8,
    }
}

#[derive(Debug, Default)]
pub struct Superframe {
    pub format: Option<AudioFormat>,
    pub units: Vec<Vec<u8>>,
}

pub struct SuperframeDecoder {
    frames: VecDeque<Vec<u8>>,
    gf: Gf,
}

impl Default for SuperframeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SuperframeDecoder {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(5),
            gf: Gf::new(),
        }
    }

    pub fn push(&mut self, frame: Vec<u8>) -> Option<Superframe> {
        self.frames.push_back(frame);
        if self.frames.len() < 5 {
            return None;
        }
        if !fire_code_ok(&self.frames[0]) {
            let mut sf: Vec<u8> = self.frames.iter().flatten().copied().collect();
            if !self.correct(&mut sf) || !fire_code_ok(&sf) {
                self.frames.pop_front();
                return None;
            }
        }
        let mut sf: Vec<u8> = self.frames.drain(..).flatten().collect();
        self.correct(&mut sf);
        Some(parse(&sf))
    }

    fn correct(&self, sf: &mut [u8]) -> bool {
        let s = sf.len() / rs::N;
        let mut ok = true;
        let mut cw = vec![0u8; rs::N];
        for c in 0..s {
            for (k, b) in cw.iter_mut().enumerate() {
                *b = sf[c + s * k];
            }
            match self.gf.correct(&mut cw) {
                Some(_) => {
                    for (k, b) in cw.iter().enumerate() {
                        sf[c + s * k] = *b;
                    }
                }
                None => ok = false,
            }
        }
        ok
    }
}

fn fire_code_ok(frame: &[u8]) -> bool {
    if frame.len() < 11 {
        return false;
    }
    let mut crc: u16 = 0;
    for &byte in &frame[2..11] {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ FIRE_POLY
            } else {
                crc << 1
            };
        }
    }
    crc == u16::from_be_bytes([frame[0], frame[1]]) && frame[2..11].iter().any(|&b| b != 0)
}

fn parse(sf: &[u8]) -> Superframe {
    let mut out = Superframe::default();
    let s = sf.len() / rs::N;
    let audio_len = s * rs::K;
    let h = sf[2];
    let format = AudioFormat {
        dac_48k: h & 0x40 != 0,
        sbr: h & 0x20 != 0,
        stereo: h & 0x10 != 0,
        ps: h & 0x08 != 0,
    };
    out.format = Some(format);
    let units = format.units();
    let mut starts = vec![3 + (12 * (units - 1)).div_ceil(8)];
    let mut bit = 24;
    for _ in 1..units {
        let byte = bit / 8;
        let v = ((sf[byte] as u16) << 8) | sf[byte + 1] as u16;
        let shift = 4 - (bit % 8);
        starts.push(((v >> shift) & 0x0fff) as usize);
        bit += 12;
    }
    starts.push(audio_len);
    for w in starts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a + 2 >= b || b > audio_len {
            continue;
        }
        let au = &sf[a..b];
        if crc16_ok(au) {
            out.units.push(au[..au.len() - 2].to_vec());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asc_for_he_aac_v2_at_48k() {
        let f = AudioFormat {
            dac_48k: true,
            sbr: true,
            ps: true,
            stereo: false,
        };
        assert_eq!(
            f.audio_specific_config(),
            vec![0x13, 0x0c, 0x56, 0xe5, 0x9d, 0x48, 0x80]
        );
        assert_eq!(f.output_channels(), 2);
    }

    #[test]
    fn asc_for_plain_aac_stereo_48k() {
        let f = AudioFormat {
            dac_48k: true,
            sbr: false,
            ps: false,
            stereo: true,
        };
        assert_eq!(f.audio_specific_config(), vec![0x11, 0x94]);
    }

    fn fire(data: &[u8]) -> [u8; 2] {
        let mut crc: u16 = 0;
        for &byte in data {
            crc ^= (byte as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ FIRE_POLY
                } else {
                    crc << 1
                };
            }
        }
        crc.to_be_bytes()
    }

    #[test]
    fn decodes_a_built_superframe_with_errors() {
        let gf = Gf::new();
        let s = 4;
        let audio_len = s * rs::K;
        let mut audio = vec![0u8; audio_len];
        audio[2] = 0x60;
        let starts = [6usize, 150, 300];
        audio[3] = (starts[1] >> 4) as u8;
        audio[4] = (((starts[1] & 0x0f) << 4) | (starts[2] >> 8)) as u8;
        audio[5] = starts[2] as u8;
        let ends = [150usize, 300, audio_len];
        for (i, (&a, &b)) in starts.iter().zip(&ends).enumerate() {
            for (k, byte) in audio[a..b - 2].iter_mut().enumerate() {
                *byte = (k as u8).wrapping_mul(3).wrapping_add(i as u8);
            }
            let crc = crate::fec::crc16(&audio[a..b - 2]) ^ 0xffff;
            audio[b - 2..b].copy_from_slice(&crc.to_be_bytes());
        }
        let f = fire(&audio[2..11]);
        audio[..2].copy_from_slice(&f);

        let mut sf = vec![0u8; s * rs::N];
        for c in 0..s {
            let data: Vec<u8> = (0..rs::K).map(|k| audio[c + s * k]).collect();
            for (k, b) in gf.encode(&data).into_iter().enumerate() {
                sf[c + s * k] = b;
            }
        }
        let clean = sf.clone();
        sf[0] ^= 0x10;
        sf[200] ^= 0xff;

        let mut dec = SuperframeDecoder::new();
        let frame_len = sf.len() / 5;
        assert!(dec.push(vec![0xaa; frame_len]).is_none());
        let mut out = None;
        for chunk in sf.chunks(frame_len) {
            out = dec.push(chunk.to_vec());
        }
        let out = out.expect("superframe");
        assert_eq!(out.units.len(), 3);
        assert_eq!(out.units[1], clean[150..298].to_vec());
    }
}
