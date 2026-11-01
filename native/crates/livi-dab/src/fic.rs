// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use crate::charset;
use crate::fec::{Puncturing, crc16_ok, descramble, pack, viterbi};

const FIC_BLOCK_BITS: usize = 2304;
const FIB_BYTES: usize = 32;
pub const ASCTY_DAB_PLUS: u8 = 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protection {
    Eep { option: u8, level: u8 },
    Uep { index: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subchannel {
    pub id: u8,
    pub start: u16,
    pub size: u16,
    pub protection: Protection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Component {
    pub subchannel: u8,
    pub ascty: u8,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketRef {
    pub scid: u16,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketComponent {
    pub subchannel: u8,
    pub address: u16,
}

pub const UA_SPI: u16 = 0x007;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Service {
    pub id: u32,
    pub label: Option<String>,
    pub short_label: Option<String>,
    pub components: Vec<Component>,
    pub packets: Vec<PacketRef>,
    pub user_apps: Vec<u16>,
}

impl Service {
    pub fn dab_plus_subchannel(&self) -> Option<u8> {
        let mut audio: Vec<&Component> = self
            .components
            .iter()
            .filter(|c| c.ascty == ASCTY_DAB_PLUS)
            .collect();
        audio.sort_by_key(|c| !c.primary);
        audio.first().map(|c| c.subchannel)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ensemble {
    pub id: Option<u16>,
    pub label: Option<String>,
    pub subchannels: BTreeMap<u8, Subchannel>,
    pub services: BTreeMap<u32, Service>,
    pub packet_components: BTreeMap<u16, PacketComponent>,
}

impl Ensemble {
    pub fn spi(&self) -> Option<(Subchannel, u16)> {
        self.services
            .values()
            .filter(|s| s.user_apps.contains(&UA_SPI))
            .find_map(|s| {
                let primary = s.packets.iter().find(|p| p.primary).or(s.packets.first())?;
                let pc = self.packet_components.get(&primary.scid)?;
                Some((*self.subchannels.get(&pc.subchannel)?, pc.address))
            })
    }

    pub fn dab_plus_services(&self) -> impl Iterator<Item = (&Service, &Subchannel)> {
        self.services.values().filter_map(|s| {
            let sub = self.subchannels.get(&s.dab_plus_subchannel()?)?;
            Some((s, sub))
        })
    }
}

pub struct FicDecoder {
    puncturing: Puncturing,
    pub ensemble: Ensemble,
    pub fibs_ok: u64,
    pub fibs_bad: u64,
}

impl Default for FicDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FicDecoder {
    pub fn new() -> Self {
        Self {
            puncturing: Puncturing {
                runs: vec![(21, 16), (3, 15)],
            },
            ensemble: Ensemble::default(),
            fibs_ok: 0,
            fibs_bad: 0,
        }
    }

    pub fn process(&mut self, soft: &[i8]) {
        for block in soft.as_chunks::<FIC_BLOCK_BITS>().0 {
            let mut bits = viterbi(&self.puncturing.depuncture(block));
            descramble(&mut bits);
            for fib in pack(&bits).as_chunks::<FIB_BYTES>().0 {
                if crc16_ok(fib) {
                    self.fibs_ok += 1;
                    parse_fib(&mut self.ensemble, &fib[..30]);
                } else {
                    self.fibs_bad += 1;
                }
            }
        }
    }
}

pub fn parse_fib(e: &mut Ensemble, fib: &[u8]) {
    let mut i = 0;
    while i < fib.len() {
        let header = fib[i];
        if header == 0xff {
            break;
        }
        let kind = header >> 5;
        let len = (header & 0x1f) as usize;
        let Some(body) = fib.get(i + 1..i + 1 + len) else {
            break;
        };
        match kind {
            0 => fig0(e, body),
            1 => fig1(e, body),
            _ => {}
        }
        i += 1 + len;
    }
}

fn fig0(e: &mut Ensemble, body: &[u8]) {
    let Some((&head, data)) = body.split_first() else {
        return;
    };
    let next_config = head & 0x80 != 0;
    let other_ensemble = head & 0x40 != 0;
    let long_sid = head & 0x20 != 0;
    if next_config || other_ensemble {
        return;
    }
    match head & 0x1f {
        0 if data.len() >= 2 => e.id = Some(u16::from_be_bytes([data[0], data[1]])),
        1 => fig0_1(e, data),
        2 => fig0_2(e, data, long_sid),
        3 => fig0_3(e, data),
        13 => fig0_13(e, data, long_sid),
        _ => {}
    }
}

fn fig0_1(e: &mut Ensemble, mut d: &[u8]) {
    while d.len() >= 3 {
        let id = d[0] >> 2;
        let start = (((d[0] & 0x03) as u16) << 8) | d[1] as u16;
        let (protection, size, used) = if d[2] & 0x80 == 0 {
            let index = d[2] & 0x3f;
            (Protection::Uep { index }, uep_size(index), 3)
        } else {
            if d.len() < 4 {
                return;
            }
            let option = (d[2] >> 4) & 0x07;
            let level = (d[2] >> 2) & 0x03;
            let size = (((d[2] & 0x03) as u16) << 8) | d[3] as u16;
            (Protection::Eep { option, level }, size, 4)
        };
        e.subchannels.insert(
            id,
            Subchannel {
                id,
                start,
                size,
                protection,
            },
        );
        d = &d[used..];
    }
}

fn uep_size(index: u8) -> u16 {
    const SIZES: [u16; 64] = [
        16, 21, 24, 29, 35, 24, 29, 35, 42, 52, 29, 35, 42, 52, 32, 42, 48, 58, 70, 40, 52, 58, 70,
        84, 48, 58, 70, 84, 104, 58, 70, 84, 104, 64, 84, 96, 116, 140, 80, 104, 116, 140, 168, 96,
        116, 140, 168, 208, 116, 140, 168, 208, 232, 128, 168, 192, 232, 280, 160, 208, 280, 192,
        280, 416,
    ];
    SIZES[index as usize]
}

fn fig0_2(e: &mut Ensemble, mut d: &[u8], long_sid: bool) {
    let sid_len = if long_sid { 4 } else { 2 };
    while d.len() > sid_len {
        let id = d[..sid_len].iter().fold(0u32, |a, &b| (a << 8) | b as u32);
        let count = (d[sid_len] & 0x0f) as usize;
        let need = sid_len + 1 + 2 * count;
        if d.len() < need {
            return;
        }
        let mut components = Vec::new();
        let mut packets = Vec::new();
        for c in d[sid_len + 1..need].as_chunks::<2>().0 {
            let primary = c[1] & 0x02 != 0;
            match c[0] >> 6 {
                0 => components.push(Component {
                    subchannel: c[1] >> 2,
                    ascty: c[0] & 0x3f,
                    primary,
                }),
                3 => packets.push(PacketRef {
                    scid: (((c[0] & 0x3f) as u16) << 6) | (c[1] >> 2) as u16,
                    primary,
                }),
                _ => {}
            }
        }
        let service = e.services.entry(id).or_insert_with(|| Service {
            id,
            ..Default::default()
        });
        service.components = components;
        service.packets = packets;
        d = &d[need..];
    }
}

fn fig0_3(e: &mut Ensemble, mut d: &[u8]) {
    while d.len() >= 5 {
        let scid = ((d[0] as u16) << 4) | (d[1] >> 4) as u16;
        let ca_org = d[1] & 0x01 != 0;
        let component = PacketComponent {
            subchannel: d[3] >> 2,
            address: (((d[3] & 0x03) as u16) << 8) | d[4] as u16,
        };
        e.packet_components.insert(scid, component);
        d = &d[(if ca_org { 7 } else { 5 }).min(d.len())..];
    }
}

fn fig0_13(e: &mut Ensemble, mut d: &[u8], long_sid: bool) {
    let sid_len = if long_sid { 4 } else { 2 };
    while d.len() > sid_len {
        let id = d[..sid_len].iter().fold(0u32, |a, &b| (a << 8) | b as u32);
        let scids = d[sid_len] >> 4;
        let count = (d[sid_len] & 0x0f) as usize;
        let mut i = sid_len + 1;
        let mut apps = Vec::new();
        for _ in 0..count {
            let Some(h) = d.get(i..i + 2) else { return };
            apps.push(((h[0] as u16) << 3) | (h[1] >> 5) as u16);
            i += 2 + (h[1] & 0x1f) as usize;
        }
        if i > d.len() {
            return;
        }
        if scids == 0 {
            let s = e.services.entry(id).or_insert_with(|| Service {
                id,
                ..Default::default()
            });
            s.user_apps = apps;
        }
        d = &d[i..];
    }
}

fn fig1(e: &mut Ensemble, body: &[u8]) {
    let Some((&head, data)) = body.split_first() else {
        return;
    };
    let charset = head >> 4;
    let other_ensemble = head & 0x08 != 0;
    if other_ensemble {
        return;
    }
    let ext = head & 0x07;
    let id_len = match ext {
        0 | 1 => 2,
        5 => 4,
        _ => return,
    };
    if data.len() < id_len + 18 {
        return;
    }
    let id = data[..id_len]
        .iter()
        .fold(0u32, |a, &b| (a << 8) | b as u32);
    let raw = &data[id_len..id_len + 16];
    let flags = u16::from_be_bytes([data[id_len + 16], data[id_len + 17]]);
    let label = charset::decode(charset, raw);
    let short: Vec<u8> = raw
        .iter()
        .enumerate()
        .filter(|(i, _)| flags & (0x8000 >> i) != 0)
        .map(|(_, &b)| b)
        .collect();
    let short = charset::decode(charset, &short);
    if ext == 0 {
        e.label = Some(label);
    } else {
        let s = e.services.entry(id).or_insert_with(|| Service {
            id,
            ..Default::default()
        });
        s.label = Some(label);
        s.short_label = Some(short);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subchannel_service_and_label() {
        let mut e = Ensemble::default();
        let mut fib = vec![0x05, 0x01, 0x14, 0x54, 0x88, 0x48];
        fib.extend_from_slice(&[0x06, 0x02, 0xd3, 0xa1, 0x01, 0x3f, 0x16]);
        let mut label = vec![0x35, 0x01, 0xd3, 0xa1];
        label.extend_from_slice(b"SWR3            ");
        label.extend_from_slice(&[0xf0, 0x00]);
        fib.extend_from_slice(&label);
        fib.push(0xff);
        parse_fib(&mut e, &fib);

        let sub = e.subchannels[&5];
        assert_eq!(sub.start, 84);
        assert_eq!(sub.size, 72);
        assert_eq!(
            sub.protection,
            Protection::Eep {
                option: 0,
                level: 2
            }
        );
        let s = &e.services[&0xd3a1];
        assert_eq!(s.label.as_deref(), Some("SWR3"));
        assert_eq!(s.short_label.as_deref(), Some("SWR3"));
        assert_eq!(s.dab_plus_subchannel(), Some(5));
        assert_eq!(e.dab_plus_services().count(), 1);
    }

    #[test]
    fn finds_the_spi_data_service() {
        let mut e = Ensemble::default();
        let mut fib = vec![0x05, 0x01, 0x79, 0xd6, 0x88, 0x12];
        fib.extend_from_slice(&[0x08, 0x22, 0xe0, 0xd1, 0x10, 0xbc, 0x01, 0xc2, 0x96]);
        fib.extend_from_slice(&[0x06, 0x03, 0x0a, 0x50, 0x3c, 0x78, 0x01]);
        fib.extend_from_slice(&[
            0x0a, 0x2d, 0xe0, 0xd1, 0x10, 0xbc, 0x01, 0x00, 0xe2, 0x01, 0x02,
        ]);
        fib.push(0xff);
        parse_fib(&mut e, &fib);

        let s = &e.services[&0xe0d1_10bc];
        assert_eq!(
            s.packets,
            [PacketRef {
                scid: 0x0a5,
                primary: true
            }]
        );
        assert_eq!(s.user_apps, [UA_SPI]);
        assert_eq!(
            e.packet_components[&0x0a5],
            PacketComponent {
                subchannel: 30,
                address: 1
            }
        );
        let (sub, address) = e.spi().unwrap();
        assert_eq!((sub.id, sub.start, sub.size, address), (30, 470, 18, 1));
    }
}
