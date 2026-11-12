// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const BANDS: usize = 24;
const FLOOR_DB: f32 = -80.0;
const MIN_FREQ: f32 = 20.0;
const MAX_FREQ: f32 = 24_000.0;
const WINDOW_SECS: f32 = 4096.0 / 48_000.0;
const STALE_PUMPS: u32 = 8;

struct Analyzer {
    rate: u32,
    hop: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    band_of: Vec<Option<usize>>,
    pending: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    power: [f32; BANDS],
    quiet: u32,
}

impl Analyzer {
    fn new(rate: u32, planner: &mut RealFftPlanner<f32>) -> Self {
        let size = ((rate as f32 * WINDOW_SECS) as usize).next_power_of_two();
        let fft = planner.plan_fft_forward(size);
        let window = (0..size)
            .map(|i| {
                0.54 - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos()
            })
            .collect();
        let (log_min, log_max) = (MIN_FREQ.log10(), MAX_FREQ.log10());
        let band_of = (0..=size / 2)
            .map(|bin| {
                let freq = bin as f32 * rate as f32 / size as f32;
                if bin == 0 || !(MIN_FREQ..=MAX_FREQ).contains(&freq) {
                    return None;
                }
                let band = ((freq.log10() - log_min) / (log_max - log_min) * BANDS as f32) as usize;
                (band < BANDS).then_some(band)
            })
            .collect();
        Self {
            rate,
            hop: size / 4,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            fft,
            window,
            band_of,
            pending: Vec::new(),
            power: [0.0; BANDS],
            quiet: 0,
        }
    }

    fn push(&mut self, samples: &[u8]) {
        self.quiet = 0;
        let (whole, _) = samples.as_chunks::<2>();
        self.pending.extend(
            whole
                .iter()
                .map(|s| f32::from(i16::from_le_bytes(*s)) / 32768.0),
        );
        let size = self.input.len();
        if self.pending.len() < size {
            return;
        }
        let start = (self.pending.len() - size) / self.hop * self.hop;
        for ((x, s), w) in self
            .input
            .iter_mut()
            .zip(&self.pending[start..])
            .zip(&self.window)
        {
            *x = s * w;
        }
        self.pending.drain(..start + self.hop);
        if self.fft.process(&mut self.input, &mut self.output).is_err() {
            return;
        }
        let half = (size / 2) as f32;
        let scale = half * half;
        self.power = [0.0; BANDS];
        for (c, band) in self.output.iter().zip(&self.band_of) {
            if let Some(b) = band {
                self.power[*b] += c.norm_sqr() / scale;
            }
        }
    }
}

fn levels(power: &[f32; BANDS]) -> [f32; BANDS] {
    power.map(|p| {
        let db = (20.0 * (p.sqrt() + 1e-12).log10()).clamp(FLOOR_DB, 0.0);
        (db - FLOOR_DB) / -FLOOR_DB
    })
}

pub struct Spectrum {
    planner: RealFftPlanner<f32>,
    streams: HashMap<u32, Analyzer>,
    silent: bool,
}

impl Spectrum {
    pub fn new() -> Self {
        Self {
            planner: RealFftPlanner::new(),
            streams: HashMap::new(),
            silent: false,
        }
    }

    pub fn push(&mut self, stream: u32, rate: u32, samples: &[u8]) {
        if rate == 0 {
            return;
        }
        if self.streams.get(&stream).is_none_or(|a| a.rate != rate) {
            self.streams
                .insert(stream, Analyzer::new(rate, &mut self.planner));
        }
        if let Some(a) = self.streams.get_mut(&stream) {
            a.push(samples);
        }
    }

    pub fn pump(&mut self) -> Option<[f32; BANDS]> {
        self.streams.retain(|_, a| {
            a.quiet += 1;
            a.quiet <= STALE_PUMPS
        });
        if self.streams.is_empty() {
            if self.silent {
                return None;
            }
            self.silent = true;
            return Some([0.0; BANDS]);
        }
        self.silent = false;
        let mut total = [0.0; BANDS];
        for a in self.streams.values() {
            for (t, p) in total.iter_mut().zip(a.power) {
                *t += p;
            }
        }
        Some(levels(&total))
    }

    pub fn clear(&mut self) {
        self.streams.clear();
        self.silent = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<u8> {
        (0..(rate as f32 * secs) as usize)
            .flat_map(|i| {
                let t = i as f32 / rate as f32;
                ((amp * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16)
                    .to_le_bytes()
            })
            .collect()
    }

    fn loudest(bands: &[f32; BANDS]) -> usize {
        (0..BANDS)
            .max_by(|a, b| bands[*a].total_cmp(&bands[*b]))
            .unwrap_or(0)
    }

    fn band_of(freq: f32) -> usize {
        ((freq.log10() - MIN_FREQ.log10()) / (MAX_FREQ.log10() - MIN_FREQ.log10()) * BANDS as f32)
            as usize
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let mut s = Spectrum::new();
        s.push(1, 48_000, &sine(1000.0, 48_000, 0.2, 0.5));
        let bands = s.pump().unwrap();
        assert_eq!(loudest(&bands), band_of(1000.0));
        assert!(bands[loudest(&bands)] > 0.5);
        assert!(bands[BANDS - 1] < 0.1);
    }

    #[test]
    fn the_streams_add_up_whatever_their_rate() {
        let mut s = Spectrum::new();
        s.push(1, 48_000, &sine(5000.0, 48_000, 0.2, 0.5));
        s.push(2, 16_000, &sine(200.0, 16_000, 0.3, 0.5));
        let both = s.pump().unwrap();
        assert!(both[band_of(5000.0)] > 0.5);
        assert!(both[band_of(200.0)] > 0.5);

        let mut alone = Spectrum::new();
        alone.push(1, 48_000, &sine(5000.0, 48_000, 0.2, 0.5));
        let one = alone.pump().unwrap();
        assert!(both[band_of(200.0)] > one[band_of(200.0)] + 0.3);
    }

    #[test]
    fn a_quiet_tap_sends_one_empty_frame_then_nothing() {
        let mut s = Spectrum::new();
        s.push(1, 48_000, &sine(1000.0, 48_000, 0.2, 0.5));
        for _ in 0..STALE_PUMPS {
            assert!(s.pump().unwrap().iter().any(|b| *b > 0.0));
        }
        assert_eq!(s.pump(), Some([0.0; BANDS]));
        assert_eq!(s.pump(), None);

        s.push(1, 48_000, &sine(1000.0, 48_000, 0.2, 0.5));
        assert!(s.pump().unwrap().iter().any(|b| *b > 0.0));
        s.clear();
        assert_eq!(s.pump(), Some([0.0; BANDS]));
    }

    #[test]
    fn a_short_tap_waits_for_a_full_window_and_a_rate_change_starts_over() {
        let mut s = Spectrum::new();
        s.push(1, 48_000, &sine(1000.0, 48_000, 0.01, 0.5));
        assert_eq!(s.pump(), Some([0.0; BANDS]));
        s.push(1, 24_000, &sine(1000.0, 24_000, 0.2, 0.5));
        assert_eq!(loudest(&s.pump().unwrap()), band_of(1000.0));
        s.push(1, 0, &[1, 2]);
        assert_eq!(s.streams[&1].rate, 24_000);
    }
}
