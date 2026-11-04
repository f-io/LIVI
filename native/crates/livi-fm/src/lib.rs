// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod fir;
mod rds;
mod stereo;

use fir::{Decimator, highpass, lowpass};
use stereo::Pilot;

pub use rds::Rds;

/// The stick's rate for FM: 1.92 MHz divides into the 240 kHz multiplex and the 48 kHz audio.
pub const SAMPLE_RATE: u32 = 1_920_000;
pub const MPX_RATE: u32 = 240_000;
pub const AUDIO_RATE: u32 = 48_000;
pub const LOWEST_KHZ: u32 = 87_500;
pub const HIGHEST_KHZ: u32 = 108_000;
pub const STEP_KHZ: u32 = 100;

const DEVIATION_HZ: f32 = 75_000.0;
const SIGNAL_EVERY: usize = MPX_RATE as usize / 2;
const STEREO_NOISE_DB: f32 = -25.0;
const BLEND_RATE: f32 = 0.0002;

pub fn frequencies_khz() -> impl Iterator<Item = u32> {
    (LOWEST_KHZ..=HIGHEST_KHZ).step_by(STEP_KHZ as usize)
}

/// `noise_db` is the multiplex power above 100 kHz, where a station leaves nothing: about 0 dB
/// without a station and far below once one captures the demodulator. `offset_hz` is the mean
/// deviation, so a station tuned beside its frequency shows how far.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Signal {
    pub level_db: f32,
    pub noise_db: f32,
    pub offset_hz: f32,
    pub stereo: bool,
    pub rds: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Interleaved stereo at 48 kHz.
    Audio(Vec<i16>),
    Signal(Signal),
    Rds(Rds),
}

#[derive(Default)]
struct Meter {
    power: f64,
    offset: f64,
    noise: f64,
    iq: usize,
    mpx: usize,
}

impl Meter {
    fn signal(&self) -> Signal {
        let db = |v: f64, n: usize| (10.0 * (v / n.max(1) as f64).max(1e-12).log10()) as f32;
        Signal {
            level_db: db(self.power, self.iq),
            noise_db: db(self.noise, self.mpx),
            offset_hz: (self.offset / self.mpx.max(1) as f64) as f32 * DEVIATION_HZ,
            stereo: false,
            rds: 0,
        }
    }
}

struct Path {
    first: Decimator,
    second: Decimator,
    out: Vec<f32>,
}

impl Path {
    fn new() -> Self {
        let rate = MPX_RATE as f32;
        let audio = AUDIO_RATE as f32;
        Self {
            first: Decimator::new(lowpass(95, 22_000.0 / rate), 5),
            second: Decimator::new(lowpass(67, 17_000.0 / audio), 1),
            out: Vec::new(),
        }
    }

    fn process(&mut self, input: &[f32], scratch: &mut Vec<f32>) {
        scratch.clear();
        self.first.process(input, scratch);
        self.out.clear();
        self.second.process(scratch, &mut self.out);
    }
}

pub struct Receiver {
    audio: bool,
    deemphasis: f32,
    first_i: Decimator,
    first_q: Decimator,
    second_i: Decimator,
    second_q: Decimator,
    prev: (f32, f32),
    noise_filter: Decimator,
    rds: rds::Demod,
    decoder: rds::Decoder,
    pilot: Pilot,
    mono: Path,
    difference: Path,
    blend: f32,
    left: f32,
    right: f32,
    total: Meter,
    window: Meter,
    raw_i: Vec<f32>,
    raw_q: Vec<f32>,
    mid_i: Vec<f32>,
    mid_q: Vec<f32>,
    ch_i: Vec<f32>,
    ch_q: Vec<f32>,
    mpx: Vec<f32>,
    scratch: Vec<f32>,
    diff: Vec<f32>,
    bits: Vec<bool>,
    said: Vec<Rds>,
}

impl Receiver {
    pub fn new(deemphasis_us: u32) -> Self {
        let rate = SAMPLE_RATE as f32;
        let first = lowpass(51, 240_000.0 / rate);
        let second = lowpass(67, 0.25);
        let mut r = Self {
            audio: false,
            deemphasis: 0.0,
            first_i: Decimator::new(first.clone(), 4),
            first_q: Decimator::new(first, 4),
            second_i: Decimator::new(second.clone(), 2),
            second_q: Decimator::new(second, 2),
            prev: (0.0, 0.0),
            noise_filter: Decimator::new(highpass(41, 84_000.0 / MPX_RATE as f32), 1),
            rds: rds::Demod::new(),
            decoder: rds::Decoder::new(),
            pilot: Pilot::new(),
            mono: Path::new(),
            difference: Path::new(),
            blend: 0.0,
            left: 0.0,
            right: 0.0,
            total: Meter::default(),
            window: Meter::default(),
            raw_i: Vec::new(),
            raw_q: Vec::new(),
            mid_i: Vec::new(),
            mid_q: Vec::new(),
            ch_i: Vec::new(),
            ch_q: Vec::new(),
            mpx: Vec::new(),
            scratch: Vec::new(),
            diff: Vec::new(),
            bits: Vec::new(),
            said: Vec::new(),
        };
        r.set_deemphasis(deemphasis_us);
        r
    }

    pub fn set_deemphasis(&mut self, us: u32) {
        let tau = us.max(1) as f32 * 1e-6;
        self.deemphasis = 1.0 - (-1.0 / (tau * AUDIO_RATE as f32)).exp();
    }

    /// Without audio the receiver only measures and reads RDS, as a scan needs.
    pub fn set_audio(&mut self, on: bool) {
        self.audio = on;
    }

    /// For a new frequency: filters, loops and RDS start over.
    pub fn reset(&mut self) {
        let (audio, deemphasis) = (self.audio, self.deemphasis);
        *self = Self::new(50);
        self.audio = audio;
        self.deemphasis = deemphasis;
    }

    /// Everything measured since the last reset.
    pub fn measured(&self) -> Signal {
        let mut s = self.total.signal();
        s.stereo = self.pilot.locked();
        s
    }

    pub fn rds_synced(&self) -> bool {
        self.decoder.synced()
    }

    pub fn pi(&self) -> Option<u16> {
        self.decoder.known_pi()
    }

    pub fn name(&self) -> Option<&str> {
        self.decoder.name()
    }

    pub fn steady_name(&self) -> Option<&str> {
        self.decoder.steady_name()
    }

    pub fn push_u8(&mut self, iq: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        self.channel(iq);
        self.demodulate();

        self.bits.clear();
        self.rds.process(&self.mpx, &mut self.bits);
        self.said.clear();
        self.decoder.push(&self.bits, &mut self.said);
        events.extend(self.said.drain(..).map(Event::Rds));

        if self.audio {
            events.push(Event::Audio(self.stereo_audio()));
        } else {
            // A scan tells a station from noise by its pilot.
            self.diff.clear();
            self.pilot.difference(&self.mpx, &mut self.diff);
        }

        if self.window.mpx >= SIGNAL_EVERY {
            let mut s = self.window.signal();
            s.stereo = self.audio && self.pilot.locked();
            s.rds = self.decoder.take_quality();
            self.window = Meter::default();
            events.push(Event::Signal(s));
        }
        events
    }

    fn channel(&mut self, iq: &[u8]) {
        self.raw_i.clear();
        self.raw_q.clear();
        for p in iq.as_chunks::<2>().0 {
            self.raw_i.push((p[0] as f32 - 127.5) / 128.0);
            self.raw_q.push((p[1] as f32 - 127.5) / 128.0);
        }
        self.mid_i.clear();
        self.mid_q.clear();
        self.first_i.process(&self.raw_i, &mut self.mid_i);
        self.first_q.process(&self.raw_q, &mut self.mid_q);
        self.ch_i.clear();
        self.ch_q.clear();
        self.second_i.process(&self.mid_i, &mut self.ch_i);
        self.second_q.process(&self.mid_q, &mut self.ch_q);
    }

    fn demodulate(&mut self) {
        let gain = MPX_RATE as f32 / (2.0 * std::f32::consts::PI * DEVIATION_HZ);
        self.mpx.clear();
        let mut power = 0.0f64;
        let mut offset = 0.0f64;
        for (&i, &q) in self.ch_i.iter().zip(&self.ch_q) {
            let (pi, pq) = self.prev;
            let re = i * pi + q * pq;
            let im = q * pi - i * pq;
            self.prev = (i, q);
            let x = im.atan2(re) * gain;
            power += (i * i + q * q) as f64;
            offset += x as f64;
            self.mpx.push(x);
        }
        self.scratch.clear();
        self.noise_filter.process(&self.mpx, &mut self.scratch);
        let noise: f64 = self.scratch.iter().map(|v| (v * v) as f64).sum();
        let n = self.mpx.len();
        for m in [&mut self.total, &mut self.window] {
            m.power += power;
            m.offset += offset;
            m.noise += noise;
            m.iq += n;
            m.mpx += n;
        }
    }

    fn stereo_audio(&mut self) -> Vec<i16> {
        self.diff.clear();
        self.pilot.difference(&self.mpx, &mut self.diff);
        self.mono.process(&self.mpx, &mut self.scratch);
        self.difference.process(&self.diff, &mut self.scratch);
        let target = if self.pilot.locked() && self.window.signal().noise_db < STEREO_NOISE_DB {
            1.0
        } else {
            0.0
        };
        let a = self.deemphasis;
        let mut out = Vec::with_capacity(self.mono.out.len() * 2);
        for (&m, &s) in self.mono.out.iter().zip(&self.difference.out) {
            self.blend += BLEND_RATE * (target - self.blend);
            let s = s * self.blend;
            self.left += a * (m + s - self.left);
            self.right += a * (m - s - self.right);
            for v in [self.left, self.right] {
                out.push((v * 32767.0).clamp(-32768.0, 32767.0) as i16);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f64 {
            let mut sum = 0.0;
            for _ in 0..4 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                sum += (self.0 >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            }
            sum
        }
    }

    struct Station {
        left_hz: f64,
        right_hz: f64,
        rds: Vec<bool>,
        offset_hz: f64,
        noise: f64,
    }

    fn differential(bits: &[bool]) -> Vec<bool> {
        let mut d = false;
        bits.iter()
            .map(|&a| {
                d ^= a;
                d
            })
            .collect()
    }

    fn synth(st: &Station, seconds: f64) -> Vec<u8> {
        let fs = SAMPLE_RATE as f64;
        let coded = differential(&st.rds);
        let mut noise = Noise(7);
        let mut phase = 0.0f64;
        let n = (seconds * fs) as usize;
        let mut out = Vec::with_capacity(2 * n);
        let tone = |hz: f64, t: f64| {
            if hz > 0.0 {
                0.5 * (2.0 * PI * hz * t).sin()
            } else {
                0.0
            }
        };
        for k in 0..n {
            let t = k as f64 / fs;
            let mut mpx = 0.0;
            if !st.rds.is_empty() || st.left_hz > 0.0 || st.right_hz > 0.0 {
                let (l, r) = (tone(st.left_hz, t), tone(st.right_hz, t));
                mpx = 0.9 * ((l + r) / 2.0 + (l - r) / 2.0 * (2.0 * PI * 38_000.0 * t).sin())
                    + 0.1 * (2.0 * PI * 19_000.0 * t).sin();
                if !coded.is_empty() {
                    let pos = t * 1187.5;
                    let bit = coded[pos as usize % coded.len()];
                    let first = pos.fract() < 0.5;
                    let sym = if bit == first { 1.0 } else { -1.0 };
                    mpx += 0.05 * sym * (2.0 * PI * 57_000.0 * t + 0.7).cos();
                }
            }
            phase += 2.0 * PI * (75_000.0 * mpx + st.offset_hz) / fs;
            let (i, q) = if st.rds.is_empty() && st.left_hz == 0.0 && st.right_hz == 0.0 {
                (0.0, 0.0)
            } else {
                (0.6 * phase.cos(), 0.6 * phase.sin())
            };
            for v in [i + st.noise * noise.next(), q + st.noise * noise.next()] {
                out.push((127.5 + 127.0 * v).clamp(0.0, 255.0) as u8);
            }
        }
        out
    }

    fn run(rx: &mut Receiver, iq: &[u8]) -> Vec<Event> {
        iq.chunks(65_536).flat_map(|c| rx.push_u8(c)).collect()
    }

    fn power(samples: &[i16], channel: usize) -> f64 {
        let v: Vec<f64> = samples
            .iter()
            .skip(channel)
            .step_by(2)
            .map(|&s| s as f64)
            .collect();
        v.iter().map(|s| s * s).sum::<f64>() / v.len() as f64
    }

    #[test]
    fn a_left_tone_stays_on_the_left() {
        let st = Station {
            left_hz: 1_000.0,
            right_hz: 0.0,
            rds: Vec::new(),
            offset_hz: 2_000.0,
            noise: 0.02,
        };
        let mut rx = Receiver::new(50);
        rx.set_audio(true);
        let audio: Vec<i16> = run(&mut rx, &synth(&st, 1.5))
            .into_iter()
            .filter_map(|e| match e {
                Event::Audio(a) => Some(a),
                _ => None,
            })
            .flatten()
            .collect();
        let tail = &audio[audio.len() / 2..];
        let (left, right) = (power(tail, 0), power(tail, 1));
        assert!(left > 1e6, "left {left}");
        assert!(
            10.0 * (left / right).log10() > 20.0,
            "left {left} right {right}"
        );
        assert!(rx.measured().stereo);
    }

    #[test]
    fn rds_names_the_station() {
        let mut bits = Vec::new();
        for _ in 0..6 {
            bits.extend(rds::tests::stream(0xd3c2, b"LIVI FM ", b"Hello RDS\r"));
        }
        let st = Station {
            left_hz: 400.0,
            right_hz: 900.0,
            rds: bits,
            offset_hz: -3_000.0,
            noise: 0.02,
        };
        let mut rx = Receiver::new(50);
        let said: Vec<Rds> = run(&mut rx, &synth(&st, 4.0))
            .into_iter()
            .filter_map(|e| match e {
                Event::Rds(r) => Some(r),
                _ => None,
            })
            .collect();
        assert!(said.contains(&Rds::Pi(0xd3c2)), "{said:?}");
        assert!(said.contains(&Rds::Name("LIVI FM".into())), "{said:?}");
        assert!(said.contains(&Rds::Text("Hello RDS".into())), "{said:?}");
    }

    #[test]
    fn a_station_quiets_the_noise_band() {
        let station = Station {
            left_hz: 1_000.0,
            right_hz: 1_000.0,
            rds: Vec::new(),
            offset_hz: 12_000.0,
            noise: 0.02,
        };
        let empty = Station {
            left_hz: 0.0,
            right_hz: 0.0,
            rds: Vec::new(),
            offset_hz: 0.0,
            noise: 0.05,
        };
        let mut rx = Receiver::new(50);
        run(&mut rx, &synth(&station, 0.3));
        let on = rx.measured();
        rx.reset();
        run(&mut rx, &synth(&empty, 0.3));
        let off = rx.measured();
        assert!(on.noise_db < -30.0, "{on:?}");
        assert!(off.noise_db > -10.0, "{off:?}");
        assert!((on.offset_hz - 12_000.0).abs() < 1_000.0, "{on:?}");
    }
}
