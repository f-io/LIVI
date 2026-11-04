// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::f32::consts::PI;

/// Blackman-windowed sinc, `cutoff` as a fraction of the sample rate. The transition band is
/// about 5.5 / `taps` wide and centred on `cutoff`.
pub fn lowpass(taps: usize, cutoff: f32) -> Vec<f32> {
    let m = (taps - 1) as f32;
    let mut h: Vec<f32> = (0..taps)
        .map(|n| {
            let x = n as f32 - m / 2.0;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * x).sin() / (PI * x)
            };
            let w = 0.42 - 0.5 * (2.0 * PI * n as f32 / m).cos()
                + 0.08 * (4.0 * PI * n as f32 / m).cos();
            sinc * w
        })
        .collect();
    let sum: f32 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    h
}

pub fn highpass(taps: usize, cutoff: f32) -> Vec<f32> {
    let mut h = lowpass(taps, cutoff);
    h.iter_mut().for_each(|v| *v = -*v);
    h[taps / 2] += 1.0;
    h
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (ac, ar) = a.as_chunks::<8>();
    let (bc, br) = b.as_chunks::<8>();
    for (x, y) in ac.iter().zip(bc) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    let mut sum: f32 = acc.iter().sum();
    for (x, y) in ar.iter().zip(br) {
        sum += x * y;
    }
    sum
}

pub struct Decimator {
    taps: Vec<f32>,
    factor: usize,
    buf: Vec<f32>,
    at: usize,
}

impl Decimator {
    pub fn new(taps: Vec<f32>, factor: usize) -> Self {
        let buf = vec![0.0; taps.len() - 1];
        Self {
            taps,
            factor,
            buf,
            at: 0,
        }
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.buf.resize(self.taps.len() - 1, 0.0);
        self.at = 0;
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        let n = self.taps.len();
        while self.at + n <= self.buf.len() {
            out.push(dot(&self.buf[self.at..self.at + n], &self.taps));
            self.at += self.factor;
        }
        let done = self.at.min(self.buf.len());
        self.buf.drain(..done);
        self.at -= done;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(taps: &[f32], f: f32) -> f32 {
        let (re, im) = taps
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(re, im), (n, h)| {
                let w = 2.0 * PI * f * n as f32;
                (re + h * w.cos(), im - h * w.sin())
            });
        (re * re + im * im).sqrt()
    }

    #[test]
    fn a_lowpass_passes_below_and_stops_above_its_band() {
        let h = lowpass(67, 0.25);
        assert!((gain(&h, 0.0) - 1.0).abs() < 1e-3);
        assert!(gain(&h, 0.2) > 0.99);
        assert!(gain(&h, 0.3) < 1e-3);
    }

    #[test]
    fn a_highpass_is_the_complement() {
        let h = highpass(41, 0.35);
        assert!(gain(&h, 0.0) < 1e-3);
        assert!(gain(&h, 0.45) > 0.99);
    }

    #[test]
    fn the_decimator_keeps_every_nth_filtered_sample() {
        let mut d = Decimator::new(vec![1.0], 4);
        let mut out = Vec::new();
        d.process(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &mut out);
        d.process(&[7.0, 8.0, 9.0], &mut out);
        assert_eq!(out, vec![1.0, 5.0, 9.0]);
    }
}
