// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::f64::consts::TAU;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::tables::*;

const ADVANCE: usize = 48;
const COARSE_SEARCH_BINS: i32 = 30;
const MIN_PEAK_RATIO: f32 = 8.0;
const LOST_AFTER: u32 = 5;
const MARGIN: usize = 4096;

pub struct Frame {
    pub fic: Vec<i8>,
    pub msc: Vec<i8>,
}

enum Lock {
    Search,
    Track { prs: usize },
}

pub struct Demodulator {
    buf: Vec<Complex32>,
    lock: Lock,
    freq_locked: bool,
    misses: u32,
    freq_hz: f64,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    prs: Vec<Complex32>,
    map: Vec<usize>,
    work: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl Default for Demodulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Demodulator {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(TU);
        let ifft = planner.plan_fft_inverse(TU);
        let scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        Self {
            buf: Vec::with_capacity(4 * FRAME),
            lock: Lock::Search,
            freq_locked: false,
            misses: 0,
            freq_hz: 0.0,
            fft,
            ifft,
            prs: phase_reference(),
            map: frequency_map().into_iter().map(bin).collect(),
            work: vec![Complex32::default(); TU],
            scratch,
        }
    }

    pub fn locked(&self) -> bool {
        matches!(self.lock, Lock::Track { .. }) && self.freq_locked
    }

    pub fn push(&mut self, iq: &[Complex32], out: &mut dyn FnMut(Frame)) {
        self.buf.extend_from_slice(iq);
        loop {
            match self.lock {
                Lock::Search => {
                    let need = FRAME + TNULL;
                    if self.buf.len() < need + MARGIN + FRAME {
                        return;
                    }
                    match find_null(&self.buf[..need]) {
                        Some(null) => self.lock = Lock::Track { prs: null + TNULL },
                        None => {
                            self.buf.drain(..FRAME);
                        }
                    }
                }
                Lock::Track { prs } => {
                    if self.buf.len() < prs + FRAME + MARGIN {
                        return;
                    }
                    match self.frame(prs) {
                        Some((frame, start)) => {
                            self.misses = 0;
                            out(frame);
                            self.advance(start + FRAME);
                        }
                        None => {
                            self.misses += 1;
                            if self.misses >= LOST_AFTER {
                                self.misses = 0;
                                self.freq_locked = false;
                                self.lock = Lock::Search;
                                self.buf.drain(..prs.min(self.buf.len()));
                            } else {
                                self.advance(prs + FRAME);
                            }
                        }
                    }
                }
            }
        }
    }

    fn advance(&mut self, next_prs: usize) {
        let keep_from = next_prs.saturating_sub(MARGIN);
        self.buf.drain(..keep_from);
        self.lock = Lock::Track {
            prs: next_prs - keep_from,
        };
    }

    fn mixed(&self, start: usize, len: usize, phase0: f64) -> Vec<Complex32> {
        let step = -TAU * self.freq_hz / SAMPLE_RATE as f64;
        self.buf[start..start + len]
            .iter()
            .enumerate()
            .map(|(n, s)| {
                let (sin, cos) = (phase0 + step * n as f64).sin_cos();
                s * Complex32::new(cos as f32, sin as f32)
            })
            .collect()
    }

    fn spectrum(&mut self, time: &[Complex32]) -> Vec<Complex32> {
        self.work.copy_from_slice(time);
        self.fft
            .process_with_scratch(&mut self.work, &mut self.scratch);
        self.work.clone()
    }

    fn prs_spectrum(&mut self, prs: usize) -> Vec<Complex32> {
        let start = prs + TG - ADVANCE;
        let t = self.mixed(start, TU, 0.0);
        self.spectrum(&t)
    }

    fn coarse_offset(&self, r: &[Complex32]) -> i32 {
        let mut best = (0, 0.0f32);
        for shift in -COARSE_SEARCH_BINS..=COARSE_SEARCH_BINS {
            let mut acc = Complex32::new(0.0, 0.0);
            for k in -767..=768 {
                if k == 0 || k == 1 {
                    continue;
                }
                let rx = r[bin(k + shift)] * r[bin(k - 1 + shift)].conj();
                let refd = self.prs[bin(k)] * self.prs[bin(k - 1)].conj();
                acc += rx * refd.conj();
            }
            let m = acc.norm();
            if m > best.1 {
                best = (shift, m);
            }
        }
        best.0
    }

    fn timing(&mut self, r: &[Complex32]) -> (i32, f32) {
        for (w, (x, p)) in self.work.iter_mut().zip(r.iter().zip(&self.prs)) {
            *w = x * p.conj();
        }
        self.ifft
            .process_with_scratch(&mut self.work, &mut self.scratch);
        let pow: Vec<f32> = self.work.iter().map(|c| c.norm_sqr()).collect();
        let (idx, peak) = pow
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |a, (i, &p)| if p > a.1 { (i, p) } else { a });
        let mean = pow.iter().sum::<f32>() / TU as f32;
        let signed = if idx > TU / 2 {
            idx as i32 - TU as i32
        } else {
            idx as i32
        };
        (signed - ADVANCE as i32, peak / mean.max(1e-12))
    }

    fn frame(&mut self, prs: usize) -> Option<(Frame, usize)> {
        let mut r = self.prs_spectrum(prs);
        if !self.freq_locked {
            let shift = self.coarse_offset(&r);
            self.freq_hz += shift as f64 * 1000.0;
            r = self.prs_spectrum(prs);
        }
        let (offset, ratio) = self.timing(&r);
        if ratio < MIN_PEAK_RATIO {
            self.freq_locked = false;
            return None;
        }
        self.freq_locked = true;
        let start = (prs as i64 + offset as i64).max(0) as usize;
        if start + SYMBOLS * TS > self.buf.len() {
            return None;
        }

        let samples = self.mixed(start, SYMBOLS * TS, 0.0);
        let mut cp = Complex32::new(0.0, 0.0);
        for s in 0..SYMBOLS {
            let sym = &samples[s * TS..(s + 1) * TS];
            for n in 0..TG {
                cp += sym[n].conj() * sym[n + TU];
            }
        }
        let residual = cp.arg() as f64 / TAU * (SAMPLE_RATE as f64 / TU as f64);
        self.freq_hz += 0.5 * residual;

        let mut prev = Vec::new();
        let mut fic = Vec::with_capacity(FIC_SYMBOLS * BITS_PER_SYMBOL);
        let mut msc = Vec::with_capacity(MSC_SYMBOLS * BITS_PER_SYMBOL);
        for s in 0..SYMBOLS {
            let w = s * TS + TG - ADVANCE;
            let spec = self.spectrum(&samples[w..w + TU]);
            if s > 0 {
                let out = if s <= FIC_SYMBOLS { &mut fic } else { &mut msc };
                self.soft_bits(&spec, &prev, out);
            }
            prev = spec;
        }
        Some((Frame { fic, msc }, start))
    }

    fn soft_bits(&self, cur: &[Complex32], prev: &[Complex32], out: &mut Vec<i8>) {
        let d: Vec<Complex32> = self.map.iter().map(|&b| cur[b] * prev[b].conj()).collect();
        let mean = d.iter().map(|c| c.re.abs() + c.im.abs()).sum::<f32>() / (2 * CARRIERS) as f32;
        let scale = 48.0 / mean.max(1e-12);
        let base = out.len();
        out.resize(base + BITS_PER_SYMBOL, 0);
        for (n, c) in d.iter().enumerate() {
            out[base + n] = (c.re * scale).clamp(-127.0, 127.0) as i8;
            out[base + CARRIERS + n] = (c.im * scale).clamp(-127.0, 127.0) as i8;
        }
    }
}

fn find_null(s: &[Complex32]) -> Option<usize> {
    let pow: Vec<f32> = s.iter().map(|c| c.norm_sqr()).collect();
    let mut win: f32 = pow[..TNULL].iter().sum();
    let (mut best, mut at) = (win, 0);
    for i in TNULL..pow.len() {
        win += pow[i] - pow[i - TNULL];
        if win < best {
            best = win;
            at = i + 1 - TNULL;
        }
    }
    let mean = pow.iter().sum::<f32>() / pow.len() as f32;
    (best / (TNULL as f32) < 0.5 * mean).then_some(at)
}
