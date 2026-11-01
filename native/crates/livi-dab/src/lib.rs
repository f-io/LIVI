// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod charset;
pub mod dabplus;
pub mod fec;
pub mod fic;
pub mod mot;
pub mod msc;
pub mod ofdm;
pub mod pad;
mod rs;
pub mod spi;
pub mod tables;

use rustfft::num_complex::Complex32;

use dabplus::SuperframeDecoder;
use fic::FicDecoder;
use msc::SubchannelDecoder;
use ofdm::Demodulator;
use pad::PadDecoder;

pub use dabplus::AudioFormat;
pub use fic::{Ensemble, Service, Subchannel};

pub const CHANNELS: [(&str, u32); 41] = [
    ("5A", 174_928_000),
    ("5B", 176_640_000),
    ("5C", 178_352_000),
    ("5D", 180_064_000),
    ("6A", 181_936_000),
    ("6B", 183_648_000),
    ("6C", 185_360_000),
    ("6D", 187_072_000),
    ("7A", 188_928_000),
    ("7B", 190_640_000),
    ("7C", 192_352_000),
    ("7D", 194_064_000),
    ("8A", 195_936_000),
    ("8B", 197_648_000),
    ("8C", 199_360_000),
    ("8D", 201_072_000),
    ("9A", 202_928_000),
    ("9B", 204_640_000),
    ("9C", 206_352_000),
    ("9D", 208_064_000),
    ("10A", 209_936_000),
    ("10N", 210_096_000),
    ("10B", 211_648_000),
    ("10C", 213_360_000),
    ("10D", 215_072_000),
    ("11A", 216_928_000),
    ("11N", 217_088_000),
    ("11B", 218_640_000),
    ("11C", 220_352_000),
    ("11D", 222_064_000),
    ("12A", 223_936_000),
    ("12N", 224_096_000),
    ("12B", 225_648_000),
    ("12C", 227_360_000),
    ("12D", 229_072_000),
    ("13A", 230_784_000),
    ("13B", 232_496_000),
    ("13C", 234_208_000),
    ("13D", 235_776_000),
    ("13E", 237_488_000),
    ("13F", 239_200_000),
];

pub fn convert_u8(iq: &[u8], out: &mut Vec<Complex32>) {
    out.clear();
    out.extend(
        iq.as_chunks::<2>()
            .0
            .iter()
            .map(|p| Complex32::new((p[0] as f32 - 127.5) / 128.0, (p[1] as f32 - 127.5) / 128.0)),
    );
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Sync { locked: bool, fic_ok: u8 },
    Ensemble(Ensemble),
    Format { format: AudioFormat, asc: Vec<u8> },
    Audio(Vec<Vec<u8>>),
    Label(String),
    Logo(spi::Logo),
}

struct SpiPath {
    address: u16,
    msc: SubchannelDecoder,
    packets: spi::Packets,
    carousel: spi::Carousel,
}

struct AudioPath {
    service: u32,
    msc: SubchannelDecoder,
    superframes: SuperframeDecoder,
    pad: PadDecoder,
    format: Option<AudioFormat>,
}

pub struct Receiver {
    demod: Demodulator,
    fic: FicDecoder,
    published: Option<Ensemble>,
    wanted: Option<u32>,
    audio: Option<AudioPath>,
    spi: Option<SpiPath>,
    frames: u64,
    fibs: (u64, u64),
    iq: Vec<Complex32>,
}

impl Default for Receiver {
    fn default() -> Self {
        Self::new()
    }
}

const FRAMES_PER_SECOND: u64 = 10;

impl Receiver {
    pub fn new() -> Self {
        Self {
            demod: Demodulator::new(),
            fic: FicDecoder::new(),
            published: None,
            wanted: None,
            audio: None,
            spi: None,
            frames: 0,
            fibs: (0, 0),
            iq: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        let wanted = self.wanted;
        *self = Self::new();
        self.wanted = wanted;
    }

    pub fn select(&mut self, service: Option<u32>) {
        self.wanted = service;
        self.audio = None;
    }

    pub fn ensemble(&self) -> &Ensemble {
        &self.fic.ensemble
    }

    pub fn locked(&self) -> bool {
        self.demod.locked()
    }

    pub fn push_u8(&mut self, iq: &[u8]) -> Vec<Event> {
        let mut samples = std::mem::take(&mut self.iq);
        convert_u8(iq, &mut samples);
        let mut frames = Vec::new();
        self.demod.push(&samples, &mut |f| frames.push(f));
        self.iq = samples;
        let mut events = Vec::new();
        for frame in frames {
            self.frame(frame, &mut events);
        }
        events
    }

    fn frame(&mut self, frame: ofdm::Frame, events: &mut Vec<Event>) {
        self.frames += 1;
        self.fic.process(&frame.fic);

        if self.frames.is_multiple_of(FRAMES_PER_SECOND) {
            let ok = self.fic.fibs_ok - self.fibs.0;
            let bad = self.fic.fibs_bad - self.fibs.1;
            self.fibs = (self.fic.fibs_ok, self.fic.fibs_bad);
            let fic_ok = (ok * 100 / (ok + bad).max(1)) as u8;
            events.push(Event::Sync {
                locked: self.demod.locked(),
                fic_ok,
            });
            if self.published.as_ref() != Some(&self.fic.ensemble) {
                self.published = Some(self.fic.ensemble.clone());
                events.push(Event::Ensemble(self.fic.ensemble.clone()));
            }
        }

        self.follow_service();
        self.follow_spi();
        for cif in frame.msc.as_chunks::<{ tables::CIF_BITS }>().0 {
            if let Some(spi) = self.spi.as_mut()
                && let Some(logical) = spi.msc.process_cif(cif)
            {
                for group in spi.packets.frame(&logical) {
                    events.extend(spi.carousel.data_group(&group).into_iter().map(Event::Logo));
                }
            }
            let Some(audio) = self.audio.as_mut() else {
                continue;
            };
            let Some(logical) = audio.msc.process_cif(cif) else {
                continue;
            };
            let Some(sf) = audio.superframes.push(logical) else {
                continue;
            };
            if let Some(format) = sf.format
                && !sf.units.is_empty()
                && audio.format != Some(format)
            {
                audio.format = Some(format);
                events.push(Event::Format {
                    format,
                    asc: format.audio_specific_config(),
                });
            }
            for unit in &sf.units {
                let Some(pad) = pad::pad_of_unit(unit) else {
                    continue;
                };
                events.extend(audio.pad.process(pad).into_iter().map(Event::Label));
            }
            if !sf.units.is_empty() {
                events.push(Event::Audio(sf.units));
            }
        }
    }

    fn follow_spi(&mut self) {
        if self.wanted.is_none() {
            self.spi = None;
            return;
        }
        let Some((sub, address)) = self.fic.ensemble.spi() else {
            return;
        };
        if let Some(s) = &self.spi
            && s.address == address
            && s.msc.subchannel() == &sub
        {
            return;
        }
        self.spi = SubchannelDecoder::new(sub).map(|msc| SpiPath {
            address,
            msc,
            packets: spi::Packets::new(address),
            carousel: spi::Carousel::new(),
        });
    }

    fn follow_service(&mut self) {
        let Some(want) = self.wanted else { return };
        let ensemble = &self.fic.ensemble;
        let Some(sub) = ensemble
            .services
            .get(&want)
            .and_then(|s| s.dab_plus_subchannel())
            .and_then(|id| ensemble.subchannels.get(&id))
        else {
            return;
        };
        if let Some(a) = &self.audio
            && a.service == want
            && a.msc.subchannel() == sub
        {
            return;
        }
        self.audio = SubchannelDecoder::new(*sub).map(|msc| AudioPath {
            service: want,
            msc,
            superframes: SuperframeDecoder::new(),
            pad: PadDecoder::new(),
            format: None,
        });
    }
}
