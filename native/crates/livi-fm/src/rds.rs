// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::f32::consts::PI;

use crate::MPX_RATE;
use crate::fir::{Decimator, lowpass};

const SUBCARRIER_STEPS: usize = 80;
const DECIMATION: usize = 20;
const RATE: f32 = MPX_RATE as f32 / DECIMATION as f32;
const BIT_RATE: f32 = 1187.5;
const LOOP_HZ: f32 = 10.0;
const TIMING_GAIN: f32 = 0.02;

/// From the multiplex to the differentially decoded data bits.
pub struct Demod {
    carrier: [(f32, f32); SUBCARRIER_STEPS],
    at: usize,
    dec_i: Decimator,
    dec_q: Decimator,
    mixed_i: Vec<f32>,
    mixed_q: Vec<f32>,
    base_i: Vec<f32>,
    base_q: Vec<f32>,
    phase: f32,
    freq: f32,
    kp: f32,
    ki: f32,
    level: f32,
    half: usize,
    window: Vec<f32>,
    ys: Vec<f32>,
    next: f32,
    power: f32,
    last_y: f32,
    last_bit: bool,
}

impl Demod {
    pub fn new() -> Self {
        // 57 kHz at 240 kHz repeats after 80 samples.
        let step = 2.0 * PI * 57_000.0 / MPX_RATE as f32;
        let carrier = std::array::from_fn(|n| {
            let (s, c) = (step * n as f32).sin_cos();
            (c, -s)
        });
        let taps = lowpass(185, 6_000.0 / MPX_RATE as f32);
        let wn = 2.0 * PI * LOOP_HZ / RATE;
        let samples_per_bit = RATE / BIT_RATE;
        let half = (samples_per_bit / 2.0).round() as usize;
        Self {
            carrier,
            at: 0,
            dec_i: Decimator::new(taps.clone(), DECIMATION),
            dec_q: Decimator::new(taps, DECIMATION),
            mixed_i: Vec::new(),
            mixed_q: Vec::new(),
            base_i: Vec::new(),
            base_q: Vec::new(),
            phase: 0.0,
            freq: 0.0,
            kp: 2.0 * 0.707 * wn,
            ki: wn * wn,
            level: 1e-3,
            half,
            window: vec![0.0; 2 * half],
            ys: Vec::new(),
            next: samples_per_bit,
            power: 1e-6,
            last_y: 0.0,
            last_bit: false,
        }
    }

    pub fn process(&mut self, mpx: &[f32], bits: &mut Vec<bool>) {
        self.mixed_i.clear();
        self.mixed_q.clear();
        for &x in mpx {
            let (c, s) = self.carrier[self.at];
            self.at = (self.at + 1) % SUBCARRIER_STEPS;
            self.mixed_i.push(x * c);
            self.mixed_q.push(x * s);
        }
        self.base_i.clear();
        self.base_q.clear();
        self.dec_i.process(&self.mixed_i, &mut self.base_i);
        self.dec_q.process(&self.mixed_q, &mut self.base_q);

        for n in 0..self.base_i.len() {
            let (zi, zq) = (self.base_i[n], self.base_q[n]);
            let (s, c) = self.phase.sin_cos();
            let i = zi * c + zq * s;
            let q = zq * c - zi * s;
            self.level += 0.001 * ((i * i + q * q).sqrt() - self.level);
            let e = if i >= 0.0 { q } else { -q } / (self.level + 1e-9);
            self.freq += self.ki * e;
            self.phase += self.freq + self.kp * e;
            self.phase = (self.phase + PI).rem_euclid(2.0 * PI) - PI;

            // Matched to one biphase bit: its first half against its second.
            self.window.rotate_left(1);
            let last = self.window.len() - 1;
            self.window[last] = i;
            let first: f32 = self.window[..self.half].iter().sum();
            let second: f32 = self.window[self.half..].iter().sum();
            self.ys.push(first - second);
        }
        self.strobe(bits);
    }

    fn strobe(&mut self, bits: &mut Vec<bool>) {
        let spb = RATE / BIT_RATE;
        while self.next + 1.0 < self.ys.len() as f32 && self.next - spb / 2.0 >= 0.0 {
            let y = interpolate(&self.ys, self.next);
            let mid = interpolate(&self.ys, self.next - spb / 2.0);
            self.power += 0.01 * (y * y - self.power);
            let e = mid * (self.last_y - y) / (self.power + 1e-12);
            self.last_y = y;
            let bit = y > 0.0;
            bits.push(bit != self.last_bit);
            self.last_bit = bit;
            self.next += spb + TIMING_GAIN * e.clamp(-1.0, 1.0) * spb;
        }
        let keep_from = ((self.next - spb).floor() as usize).saturating_sub(2);
        if keep_from > 0 {
            self.ys.drain(..keep_from.min(self.ys.len()));
            self.next -= keep_from as f32;
        }
    }
}

fn interpolate(ys: &[f32], t: f32) -> f32 {
    let i = t.floor() as usize;
    let f = t - i as f32;
    ys[i] * (1.0 - f) + ys[i + 1] * f
}

const POLY: u32 = 0x5b9;
const OFFSET_A: u16 = 0x0fc;
const OFFSET_B: u16 = 0x198;
const OFFSET_C: u16 = 0x168;
const OFFSET_CP: u16 = 0x350;
const OFFSET_D: u16 = 0x1b4;
const BLOCK_BITS: u32 = 26;
const MAX_BURST: usize = 2;
const CONFIRMED_AFTER: u32 = 4;
const LOST_AT: u32 = 8;

pub fn syndrome(mut word: u32) -> u16 {
    for i in (10..26).rev() {
        if word & (1 << i) != 0 {
            word ^= POLY << (i - 10);
        }
    }
    (word & 0x3ff) as u16
}

fn offsets_at(position: usize) -> &'static [u16] {
    match position {
        0 => &[OFFSET_A],
        1 => &[OFFSET_B],
        2 => &[OFFSET_C, OFFSET_CP],
        _ => &[OFFSET_D],
    }
}

fn position_of(offset: u16) -> Option<usize> {
    (0..4).find(|&p| offsets_at(p).contains(&offset))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rds {
    Pi(u16),
    Pty(u8),
    Name(String),
    Text(String),
}

const NAMES_KEPT: usize = 32;
const MIN_OVERLAP: usize = 3;
const TITLE_MAX: usize = 128;

struct Sync {
    position: usize,
    bits: u32,
    clean: u32,
    bad: u16,
}

/// Finds the 26-bit blocks and reads programme identification, type, name and radio text.
pub struct Decoder {
    word: u32,
    seen: u64,
    candidate: Option<(u64, usize)>,
    sync: Option<Sync>,
    bursts: HashMap<u16, u32>,
    group: [Option<u16>; 4],
    c_prime: bool,
    good: u32,
    total: u32,
    pi: Option<u16>,
    pi_known: Option<u16>,
    pty_said: Option<u8>,
    ps: [u8; 8],
    ps_segments: u8,
    ps_last: Option<[u8; 8]>,
    name_said: Option<String>,
    names: Vec<(String, u32)>,
    title: Option<Vec<char>>,
    title_piece: Option<String>,
    rt_heard: bool,
    rt: [u8; 64],
    rt_segments: u16,
    rt_ab: Option<bool>,
    rt_len: Option<usize>,
    text_said: Option<String>,
}

impl Decoder {
    pub fn new() -> Self {
        let mut bursts = HashMap::new();
        for len in 1..=MAX_BURST {
            let middle = if len > 2 { 1u32 << (len - 2) } else { 1 };
            for inner in 0..middle {
                let pattern = if len == 1 {
                    1
                } else {
                    (1 << (len - 1)) | (inner << 1) | 1
                };
                for shift in 0..=(BLOCK_BITS as usize - len) {
                    let e = pattern << shift;
                    bursts.entry(syndrome(e)).or_insert(e);
                }
            }
        }
        Self {
            word: 0,
            seen: 0,
            candidate: None,
            sync: None,
            bursts,
            group: [None; 4],
            c_prime: false,
            good: 0,
            total: 0,
            pi: None,
            pi_known: None,
            pty_said: None,
            ps: [b' '; 8],
            ps_segments: 0,
            ps_last: None,
            name_said: None,
            names: Vec::new(),
            title: None,
            title_piece: None,
            rt_heard: false,
            rt: [b' '; 64],
            rt_segments: 0,
            rt_ab: None,
            rt_len: None,
            text_said: None,
        }
    }

    pub fn synced(&self) -> bool {
        self.sync.is_some()
    }

    /// Share of good blocks since the last call, in percent.
    pub fn take_quality(&mut self) -> u8 {
        let q = (self.good * 100).checked_div(self.total).unwrap_or(0) as u8;
        (self.good, self.total) = (0, 0);
        q
    }

    pub fn known_pi(&self) -> Option<u16> {
        self.pi_known
    }

    pub fn name(&self) -> Option<&str> {
        self.name_said.as_deref()
    }

    /// The name while it is the only one heard. A station scrolling titles through its name
    /// has none: a short listen cannot tell its name from a title.
    pub fn steady_name(&self) -> Option<&str> {
        match self.names.as_slice() {
            [(name, _)] => Some(name),
            _ => None,
        }
    }

    pub fn push(&mut self, bits: &[bool], out: &mut Vec<Rds>) {
        for &bit in bits {
            self.word = ((self.word << 1) | bit as u32) & ((1 << BLOCK_BITS) - 1);
            self.seen += 1;
            match self.sync.as_mut() {
                None => self.search(),
                Some(sync) => {
                    sync.bits += 1;
                    if sync.bits == BLOCK_BITS {
                        sync.bits = 0;
                        self.next_block(out);
                    }
                }
            }
        }
    }

    fn search(&mut self) {
        let Some(position) = position_of(syndrome(self.word)) else {
            return;
        };
        if let Some((at, was)) = self.candidate {
            let gap = self.seen - at;
            if gap.is_multiple_of(BLOCK_BITS as u64)
                && gap <= 6 * BLOCK_BITS as u64
                && (was + (gap / BLOCK_BITS as u64) as usize) % 4 == position
            {
                self.sync = Some(Sync {
                    position: (position + 1) % 4,
                    bits: 0,
                    clean: 0,
                    bad: 0,
                });
                self.group = [None; 4];
                return;
            }
        }
        self.candidate = Some((self.seen, position));
    }

    fn next_block(&mut self, out: &mut Vec<Rds>) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        let position = sync.position;
        sync.position = (position + 1) % 4;
        let s = syndrome(self.word);
        // A sync found in noise holds on to corrected blocks, so corrections count only once
        // clean blocks have confirmed the sync.
        let confirmed = sync.clean >= CONFIRMED_AFTER;
        let data = offsets_at(position).iter().find_map(|&offset| {
            if s == offset {
                return Some(((self.word >> 10) as u16, offset, true));
            }
            let e = self.bursts.get(&(s ^ offset)).filter(|_| confirmed)?;
            Some((((self.word ^ e) >> 10) as u16, offset, false))
        });
        self.total += 1;
        sync.bad = (sync.bad << 1) | data.is_none() as u16;
        match data {
            Some((value, offset, clean)) => {
                self.good += 1;
                if clean {
                    sync.clean += 1;
                }
                if position == 0 {
                    self.group = [None; 4];
                    self.c_prime = false;
                }
                self.group[position] = Some(value);
                if position == 2 {
                    self.c_prime = offset == OFFSET_CP;
                }
            }
            None => {
                if sync.bad.count_ones() >= LOST_AT {
                    self.sync = None;
                    self.candidate = None;
                }
            }
        }
        if position == 3 {
            let group = std::mem::take(&mut self.group);
            self.group(group, out);
        }
    }

    fn group(&mut self, [a, b, c, d]: [Option<u16>; 4], out: &mut Vec<Rds>) {
        let pi = a.or(if self.c_prime { c } else { None });
        if let Some(pi) = pi {
            if self.pi == Some(pi) && self.pi_known != Some(pi) {
                self.pi_known = Some(pi);
                out.push(Rds::Pi(pi));
                self.clear_texts();
            }
            self.pi = Some(pi);
        }
        let Some(b) = b else { return };
        if self.pi_known.is_none() {
            return;
        }
        let kind = b >> 12;
        let version_b = b & 0x0800 != 0;
        let pty = ((b >> 5) & 0x1f) as u8;
        if self.pty_said != Some(pty) {
            self.pty_said = Some(pty);
            out.push(Rds::Pty(pty));
        }
        match kind {
            0 => {
                if let Some(d) = d {
                    self.name_segment((b & 3) as usize, d, out);
                }
            }
            2 => {
                let ab = b & 0x10 != 0;
                if self.rt_ab.is_some_and(|was| was != ab) {
                    self.rt = [b' '; 64];
                    self.rt_segments = 0;
                    self.rt_len = None;
                }
                self.rt_ab = Some(ab);
                let segment = (b & 0x0f) as usize;
                match (version_b, c, d) {
                    (false, Some(c), Some(d)) => {
                        self.text_segment(segment * 4, &[c, d], 64, out);
                    }
                    (true, _, Some(d)) => self.text_segment(segment * 2, &[d], 32, out),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn clear_texts(&mut self) {
        self.ps_segments = 0;
        self.ps_last = None;
        self.name_said = None;
        self.names.clear();
        self.title = None;
        self.title_piece = None;
        self.rt_heard = false;
        self.rt = [b' '; 64];
        self.rt_segments = 0;
        self.rt_ab = None;
        self.rt_len = None;
        self.text_said = None;
    }

    fn name_segment(&mut self, segment: usize, d: u16, out: &mut Vec<Rds>) {
        self.ps[segment * 2..segment * 2 + 2].copy_from_slice(&d.to_be_bytes());
        self.ps_segments |= 1 << segment;
        if self.ps_segments != 0x0f {
            return;
        }
        self.ps_segments = 0;
        let ps = self.ps;
        // A name counts once it came twice in a row, so a corrupted character stays out.
        if self.ps_last == Some(ps) {
            let name = livi_dab::charset::decode(0, &ps);
            if !name.is_empty() {
                self.heard_name(name, out);
            }
        }
        self.ps_last = Some(ps);
    }

    /// Many stations scroll the title through the name field. Their own name stays on air
    /// longest, so the name repeated most often is the station's.
    fn heard_name(&mut self, name: String, out: &mut Vec<Rds>) {
        let count = match self.names.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => {
                entry.1 += 1;
                entry.1
            }
            None => {
                if self.names.len() >= NAMES_KEPT
                    && let Some(at) = self
                        .names
                        .iter()
                        .enumerate()
                        .filter(|(_, (n, _))| Some(n) != self.name_said.as_ref())
                        .min_by_key(|(_, (_, c))| *c)
                        .map(|(i, _)| i)
                {
                    self.names.swap_remove(at);
                }
                self.names.push((name.clone(), 1));
                1
            }
        };
        let leading = self
            .name_said
            .as_ref()
            .and_then(|said| self.names.iter().find(|(n, _)| n == said))
            .map_or(0, |(_, c)| *c);
        if self.name_said.as_deref() != Some(&name) && count > leading {
            self.name_said = Some(name.clone());
            self.title = None;
            out.push(Rds::Name(name.clone()));
        }
        if self.name_said.as_deref() == Some(&name) {
            self.title_done(out);
        } else {
            self.title_piece_heard(&name);
        }
    }

    /// The station's name closes a scrolled title. Collecting starts only after it, so a
    /// title tuned into halfway never shows.
    fn title_done(&mut self, out: &mut Vec<Rds>) {
        let title: String = self
            .title
            .replace(Vec::new())
            .unwrap_or_default()
            .into_iter()
            .collect();
        self.title_piece = None;
        if title.is_empty() || self.rt_heard || self.text_said.as_deref() == Some(&title) {
            return;
        }
        self.text_said = Some(title.clone());
        out.push(Rds::Text(title));
    }

    /// Pieces come word by word or slide on a character at a time; a sliding one overlaps
    /// what came before.
    fn title_piece_heard(&mut self, piece: &str) {
        let Some(title) = self.title.as_mut() else {
            return;
        };
        if self.title_piece.as_deref() == Some(piece) {
            return;
        }
        self.title_piece = Some(piece.to_string());
        let piece: Vec<char> = piece.trim().chars().collect();
        if piece.is_empty() {
            return;
        }
        let overlap = (MIN_OVERLAP..piece.len())
            .rev()
            .find(|&k| title.ends_with(&piece[..k]))
            .unwrap_or(0);
        if overlap == 0 && !title.is_empty() {
            title.push(' ');
        }
        title.extend_from_slice(&piece[overlap..]);
        if title.len() > TITLE_MAX {
            self.title = None;
        }
    }

    fn text_segment(&mut self, at: usize, words: &[u16], max: usize, out: &mut Vec<Rds>) {
        for (k, w) in words.iter().enumerate() {
            for (j, byte) in w.to_be_bytes().into_iter().enumerate() {
                let p = at + k * 2 + j;
                if p >= max {
                    continue;
                }
                self.rt[p] = byte;
                if byte == 0x0d {
                    self.rt_len = Some(self.rt_len.map_or(p, |l| l.min(p)));
                }
            }
        }
        let per = words.len() * 2;
        self.rt_segments |= 1 << (at / per);
        let len = self.rt_len.unwrap_or(max);
        let needed = len.div_ceil(per);
        if (0..needed).any(|s| self.rt_segments & (1 << s) == 0) {
            return;
        }
        let text = livi_dab::charset::decode(0, &self.rt[..len]);
        if !text.is_empty() && self.text_said.as_deref() != Some(&text) {
            self.rt_heard = true;
            self.text_said = Some(text.clone());
            out.push(Rds::Text(text));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn block(data: u16, offset: u16) -> u32 {
        let word = (data as u32) << 10;
        word | (syndrome(word) ^ offset) as u32
    }

    fn bits_of(word: u32) -> Vec<bool> {
        (0..26).rev().map(|i| word & (1 << i) != 0).collect()
    }

    pub(crate) fn group_bits(pi: u16, b: u16, c: u16, d: u16) -> Vec<bool> {
        [
            block(pi, OFFSET_A),
            block(b, OFFSET_B),
            block(c, OFFSET_C),
            block(d, OFFSET_D),
        ]
        .into_iter()
        .flat_map(bits_of)
        .collect()
    }

    #[test]
    fn a_clean_block_has_its_offset_as_syndrome() {
        for offset in [OFFSET_A, OFFSET_B, OFFSET_C, OFFSET_CP, OFFSET_D] {
            assert_eq!(syndrome(block(0x1234, offset)), offset);
        }
    }

    pub(crate) fn stream(pi: u16, name: &[u8; 8], text: &[u8]) -> Vec<bool> {
        let mut bits = Vec::new();
        for _ in 0..3 {
            for seg in 0..4u16 {
                let d = u16::from_be_bytes([name[seg as usize * 2], name[seg as usize * 2 + 1]]);
                bits.extend(group_bits(pi, (10 << 5) | seg, 0, d));
            }
            for seg in 0..text.len().div_ceil(4) {
                let ch = |i: usize| text.get(seg * 4 + i).copied().unwrap_or(b' ');
                let c = u16::from_be_bytes([ch(0), ch(1)]);
                let d = u16::from_be_bytes([ch(2), ch(3)]);
                bits.extend(group_bits(pi, (2 << 12) | (10 << 5) | seg as u16, c, d));
            }
        }
        bits
    }

    #[test]
    fn groups_give_identification_name_and_text() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let mut bits = vec![true, false, true];
        bits.extend(stream(0xd3c2, b"LIVI FM ", b"Hello RDS\r"));
        dec.push(&bits, &mut out);
        assert!(out.contains(&Rds::Pi(0xd3c2)));
        assert!(out.contains(&Rds::Pty(10)));
        assert!(out.contains(&Rds::Name("LIVI FM".into())));
        assert!(out.contains(&Rds::Text("Hello RDS".into())));
    }

    fn names(pi: u16, shown: &[(&[u8; 8], usize)]) -> Vec<bool> {
        let mut bits = Vec::new();
        for (name, times) in shown {
            for _ in 0..*times {
                for seg in 0..4u16 {
                    let at = seg as usize * 2;
                    let d = u16::from_be_bytes([name[at], name[at + 1]]);
                    bits.extend(group_bits(pi, (10 << 5) | seg, 0, d));
                }
            }
        }
        bits
    }

    fn said(out: &[Rds]) -> Vec<&str> {
        out.iter()
            .filter_map(|r| match r {
                Rds::Name(n) => Some(n.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_title_scrolled_through_the_name_leaves_the_name() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let cycle: &[(&[u8; 8], usize)] = &[
            (b"RADIO HH", 6),
            (b"ALLE    ", 3),
            (b"FARBEN  ", 3),
            (b"RADIO HH", 6),
            (b"ALLE    ", 3),
            (b"FARBEN  ", 3),
        ];
        dec.push(&names(0xd3c2, cycle), &mut out);
        assert_eq!(said(&out), ["RADIO HH"]);
        assert_eq!(dec.name(), Some("RADIO HH"));
    }

    #[test]
    fn only_a_name_heard_alone_is_steady() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let (mut named, mut steady) = (None, None);
        for rep in 0..8 {
            dec.push(&names(0xd3c2, &[(b"NDR 2   ", 1)]), &mut out);
            named = named.or(dec.name().map(|_| rep));
            steady = steady.or(dec.steady_name().map(|_| rep));
        }
        assert_eq!(dec.steady_name(), Some("NDR 2"));
        assert_eq!(steady, named);

        let mut dec = Decoder::new();
        let shown: &[(&[u8; 8], usize)] = &[(b"IF I WER", 4), (b"RADIO HH", 4)];
        dec.push(&names(0xd3c2, shown), &mut out);
        assert_eq!(dec.steady_name(), None, "a title went through");
    }

    fn texts(out: &[Rds]) -> Vec<&str> {
        out.iter()
            .filter_map(|r| match r {
                Rds::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_title_paged_through_the_name_comes_out_whole() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let cycle: &[(&[u8; 8], usize)] = &[
            (b"FARBEN  ", 3),
            (b"RADIO HH", 6),
            (b"ALLE    ", 3),
            (b"FARBEN  ", 3),
            (b"RADIO HH", 6),
            (b"MAJESTIC", 3),
            (b"RADIO HH", 6),
        ];
        dec.push(&names(0xd3c2, cycle), &mut out);
        assert_eq!(
            texts(&out),
            ["ALLE FARBEN", "MAJESTIC"],
            "the half title before is dropped"
        );
    }

    #[test]
    fn a_title_sliding_through_the_name_comes_out_whole() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let cycle: &[(&[u8; 8], usize)] = &[
            (b"RADIO HH", 6),
            (b"ALLE FAR", 3),
            (b"LLE FARB", 3),
            (b"LE FARBE", 3),
            (b"E FARBEN", 3),
            (b"RADIO HH", 6),
        ];
        dec.push(&names(0xd3c2, cycle), &mut out);
        assert_eq!(texts(&out), ["ALLE FARBEN"]);
    }

    #[test]
    fn radio_text_beats_a_scrolled_title() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let mut bits = stream(0xd3c2, b"RADIO HH", b"Alle Farben - From disco to disco\r");
        bits.extend(names(
            0xd3c2,
            &[(b"ALLE    ", 3), (b"FARBEN  ", 3), (b"RADIO HH", 6)],
        ));
        dec.push(&bits, &mut out);
        assert_eq!(texts(&out), ["Alle Farben - From disco to disco"]);
    }

    #[test]
    fn the_name_wins_over_a_title_that_came_first() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let cycle: &[(&[u8; 8], usize)] = &[
            (b"ALLE    ", 3),
            (b"RADIO HH", 6),
            (b"FARBEN  ", 3),
            (b"ALLE    ", 3),
            (b"RADIO HH", 6),
        ];
        dec.push(&names(0xd3c2, cycle), &mut out);
        assert_eq!(said(&out), ["ALLE", "RADIO HH"]);
    }

    #[test]
    fn a_two_bit_burst_is_corrected() {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        let mut bits = stream(0xd3c2, b"LIVI FM ", b"Hi\r");
        let n = bits.len();
        for k in (0..n).step_by(150) {
            if k + 1 < n {
                bits[k] = !bits[k];
                bits[k + 1] = !bits[k + 1];
            }
        }
        dec.push(&bits, &mut out);
        assert!(out.contains(&Rds::Name("LIVI FM".into())));
    }
}
