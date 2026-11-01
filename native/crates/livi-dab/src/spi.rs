// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{BTreeMap, HashMap};

use crate::fec::crc16_ok;
use crate::mot::{self, DG_BODY, DG_DIRECTORY, Header, Segments};

const PACKET_SIZES: [usize; 4] = [24, 48, 72, 96];
const MAX_HELD_BYTES: usize = 2 << 20;
const SI_TYPE: (u8, u16) = (7, 0);
const LOGO_UNRESTRICTED: u8 = 0x02;
const LOGO_SQUARE: u8 = 0x04;
const WANTED_SIDE: i32 = 128;

pub struct Packets {
    address: u16,
    group: Vec<u8>,
    continuity: Option<u8>,
    open: bool,
}

impl Packets {
    pub fn new(address: u16) -> Self {
        Self {
            address,
            group: Vec::new(),
            continuity: None,
            open: false,
        }
    }

    pub fn frame(&mut self, lf: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 3 <= lf.len() {
            let len = PACKET_SIZES[(lf[i] >> 6) as usize];
            let Some(p) = lf.get(i..i + len) else { break };
            i += len;
            if !crc16_ok(p) {
                continue;
            }
            let address = (((p[0] & 0x03) as u16) << 8) | p[1] as u16;
            if address != self.address {
                continue;
            }
            let continuity = (p[0] >> 4) & 0x03;
            let (first, last) = (p[0] & 0x08 != 0, p[0] & 0x04 != 0);
            let useful = ((p[2] & 0x7f) as usize).min(len - 5);
            let follows = self.continuity.is_some_and(|c| (c + 1) % 4 == continuity);
            self.continuity = Some(continuity);
            if first {
                self.group.clear();
                self.open = true;
            } else if !self.open || !follows {
                self.open = false;
                continue;
            }
            self.group.extend_from_slice(&p[3..3 + useful]);
            if last {
                self.open = false;
                out.push(std::mem::take(&mut self.group));
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiLogo {
    pub service: u32,
    pub url: String,
    pub kind: Option<u8>,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logo {
    pub service: u32,
    pub mime: &'static str,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct Carousel {
    directory: Segments,
    directory_transport: Option<u16>,
    entries: HashMap<u16, Header>,
    bodies: HashMap<u16, Segments>,
    files: HashMap<String, (Header, Vec<u8>)>,
    wanted: BTreeMap<u32, SiLogo>,
    sent: HashMap<u32, String>,
}

impl Carousel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn data_group(&mut self, g: &[u8]) -> Vec<Logo> {
        let Some(dg) = mot::data_group(g) else {
            return Vec::new();
        };
        match dg.kind {
            DG_DIRECTORY => {
                if self.directory_transport != Some(dg.transport) {
                    self.directory = Segments::default();
                    self.directory_transport = Some(dg.transport);
                }
                self.directory.add(dg.segment, dg.last, dg.data);
                if let Some(entries) = self
                    .directory
                    .complete()
                    .as_deref()
                    .and_then(mot::directory)
                {
                    self.entries = entries.into_iter().collect();
                    let known = &self.entries;
                    self.bodies.retain(|tid, _| known.contains_key(tid));
                }
            }
            DG_BODY => {
                let held: usize = self.bodies.values().map(Segments::len).sum();
                if held > MAX_HELD_BYTES && !self.bodies.contains_key(&dg.transport) {
                    return Vec::new();
                }
                let body = self.bodies.entry(dg.transport).or_default();
                body.add(dg.segment, dg.last, dg.data);
            }
            _ => return Vec::new(),
        }
        self.finish()
    }

    fn finish(&mut self) -> Vec<Logo> {
        let done: Vec<u16> = self
            .bodies
            .iter()
            .filter(|(tid, b)| self.entries.contains_key(tid) && b.complete().is_some())
            .map(|(&tid, _)| tid)
            .collect();
        for tid in done {
            let (Some(body), Some(h)) = (self.bodies.remove(&tid), self.entries.get(&tid)) else {
                continue;
            };
            let Some(data) = body.complete().filter(|d| d.len() == h.body_size) else {
                continue;
            };
            if (h.content_type, h.subtype) == SI_TYPE {
                for logo in service_logos(&data) {
                    let better = self
                        .wanted
                        .get(&logo.service)
                        .is_none_or(|w| rank(&logo) < rank(w));
                    if better {
                        self.wanted.insert(logo.service, logo);
                    }
                }
            } else if let Some(name) = h.name.clone().filter(|_| h.mime().is_some()) {
                self.files.insert(name, (h.clone(), data));
            }
        }
        let mut out = Vec::new();
        for (service, logo) in &self.wanted {
            if self.sent.get(service) == Some(&logo.url) {
                continue;
            }
            let Some((h, data)) = self.files.get(&logo.url) else {
                continue;
            };
            let Some(mime) = h.mime() else { continue };
            self.sent.insert(*service, logo.url.clone());
            out.push(Logo {
                service: *service,
                mime,
                data: data.clone(),
            });
        }
        out
    }
}

fn rank(l: &SiLogo) -> (u8, i32) {
    let square = l.width == l.height && l.width > 0;
    match l.kind {
        Some(LOGO_UNRESTRICTED) if square => (0, (l.width as i32 - WANTED_SIDE).abs()),
        Some(LOGO_SQUARE) => (1, 0),
        _ if square => (2, (l.width as i32 - WANTED_SIDE).abs()),
        _ => (3, 0),
    }
}

fn tlv(d: &[u8]) -> Option<(u8, &[u8], usize)> {
    let tag = *d.first()?;
    let (len, head) = match *d.get(1)? {
        0xfe => (u16::from_be_bytes([*d.get(2)?, *d.get(3)?]) as usize, 4),
        0xff => (
            u32::from_be_bytes([0, *d.get(2)?, *d.get(3)?, *d.get(4)?]) as usize,
            5,
        ),
        n => (n as usize, 2),
    };
    Some((tag, d.get(head..head + len)?, head + len))
}

fn items(mut d: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    while let Some((tag, value, used)) = tlv(d) {
        out.push((tag, value));
        d = &d[used..];
    }
    out
}

const EL_SERVICE_INFORMATION: u8 = 0x03;
const EL_MEDIA_DESCRIPTION: u8 = 0x13;
const EL_ENSEMBLE: u8 = 0x26;
const EL_SERVICE: u8 = 0x28;
const EL_BEARER: u8 = 0x29;
const EL_MULTIMEDIA: u8 = 0x2b;

pub fn service_logos(si: &[u8]) -> Vec<SiLogo> {
    let mut out = Vec::new();
    for (tag, value) in items(si) {
        if tag == EL_SERVICE_INFORMATION {
            services(value, &mut out);
        }
    }
    out
}

fn services(d: &[u8], out: &mut Vec<SiLogo>) {
    for (tag, value) in items(d) {
        match tag {
            EL_ENSEMBLE => services(value, out),
            EL_SERVICE => service(value, out),
            _ => {}
        }
    }
}

fn service(d: &[u8], out: &mut Vec<SiLogo>) {
    let children = items(d);
    let ids: Vec<u32> = children
        .iter()
        .filter(|(tag, _)| *tag == EL_BEARER)
        .filter_map(|(_, b)| {
            items(b)
                .into_iter()
                .find(|(t, _)| *t == 0x80)
                .and_then(|(_, v)| bearer_sid(v))
        })
        .collect();
    let Some(&sid) = ids.first() else { return };
    for (tag, value) in &children {
        if *tag != EL_MEDIA_DESCRIPTION {
            continue;
        }
        for (t, m) in items(value) {
            if t == EL_MULTIMEDIA
                && let Some(logo) = multimedia(sid, m)
            {
                out.push(logo);
            }
        }
    }
}

fn multimedia(service: u32, d: &[u8]) -> Option<SiLogo> {
    let mut logo = SiLogo {
        service,
        url: String::new(),
        kind: None,
        width: 0,
        height: 0,
    };
    let u16_of = |v: &[u8]| {
        v.get(..2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .unwrap_or(0)
    };
    for (tag, v) in items(d) {
        match tag {
            0x82 => logo.url = String::from_utf8_lossy(v).into_owned(),
            0x83 => logo.kind = v.first().copied(),
            0x84 => logo.width = u16_of(v),
            0x85 => logo.height = u16_of(v),
            _ => {}
        }
    }
    if logo.kind == Some(LOGO_SQUARE) && logo.width == 0 {
        (logo.width, logo.height) = (32, 32);
    }
    (!logo.url.is_empty()).then_some(logo)
}

fn bearer_sid(id: &[u8]) -> Option<u32> {
    let flags = *id.first()?;
    let ensemble = flags & 0x40 != 0;
    let long = flags & 0x10 != 0;
    let at = 1 + if ensemble { 3 } else { 0 };
    let n = if long { 4 } else { 2 };
    Some(
        id.get(at..at + n)?
            .iter()
            .fold(0u32, |a, &b| (a << 8) | b as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fec::crc16;
    use crate::mot::tests::{group, mot_header};

    fn packet(address: u16, continuity: u8, first: bool, last: bool, data: &[u8]) -> Vec<u8> {
        let mut p = vec![
            (continuity << 4)
                | (u8::from(first) << 3)
                | (u8::from(last) << 2)
                | (address >> 8) as u8,
            address as u8,
            data.len() as u8,
        ];
        p.extend_from_slice(data);
        p.resize(22, 0);
        let c = crc16(&p) ^ 0xffff;
        p.extend_from_slice(&c.to_be_bytes());
        p
    }

    #[test]
    fn packets_of_one_address_make_a_data_group() {
        let mut lf = packet(5, 0, true, false, &[1; 19]);
        lf.extend(packet(9, 0, true, true, &[7; 3]));
        lf.extend(packet(5, 1, false, true, &[2; 4]));
        let mut broken = packet(5, 2, true, true, &[3]);
        broken[4] ^= 1;
        lf.extend(broken);
        let mut p = Packets::new(5);
        let groups = p.frame(&lf);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 23);
        assert_eq!(groups[0][19..], [2, 2, 2, 2]);

        let mut gap = packet(5, 0, true, false, &[1; 4]);
        gap.extend(packet(5, 2, false, true, &[2; 4]));
        assert!(p.frame(&gap).is_empty());
        assert!(p.frame(&packet(5, 3, false, true, &[2])).is_empty());
    }

    fn el(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        if value.len() < 0xfe {
            v.push(value.len() as u8);
        } else {
            v.push(0xfe);
            v.extend_from_slice(&(value.len() as u16).to_be_bytes());
        }
        v.extend_from_slice(value);
        v
    }

    fn logo_el(url: &str, kind: u8, side: Option<u16>) -> Vec<u8> {
        let mut attrs = el(0x82, url.as_bytes());
        attrs.extend(el(0x83, &[kind]));
        if let Some(s) = side {
            attrs.extend(el(0x84, &s.to_be_bytes()));
            attrs.extend(el(0x85, &s.to_be_bytes()));
        }
        el(EL_MULTIMEDIA, &attrs)
    }

    fn service_el(sid: u16, logos: &[Vec<u8>]) -> Vec<u8> {
        let bearer = el(
            EL_BEARER,
            &el(0x80, &[0x40, 0xe0, 0x10, 0xbc, (sid >> 8) as u8, sid as u8]),
        );
        let mut s = el(0x80, &[0, 1]);
        s.extend(el(0x10, &el(0x01, b"Dlf")));
        s.extend(el(EL_MEDIA_DESCRIPTION, &logos.concat()));
        s.extend(bearer);
        el(EL_SERVICE, &s)
    }

    fn si() -> Vec<u8> {
        let a = service_el(
            0xd210,
            &[
                logo_el("d210_32.png", LOGO_SQUARE, None),
                logo_el("d210_320.png", LOGO_UNRESTRICTED, Some(320)),
                logo_el("d210_128.png", LOGO_UNRESTRICTED, Some(128)),
            ],
        );
        let b = service_el(0xd220, &[logo_el("d220_32.png", LOGO_SQUARE, None)]);
        let ensemble = el(EL_ENSEMBLE, &[el(0x80, &[0xe0, 0x10, 0xbc]), a, b].concat());
        el(EL_SERVICE_INFORMATION, &ensemble)
    }

    #[test]
    fn service_information_names_logos_per_service() {
        let logos = service_logos(&si());
        assert_eq!(logos.len(), 4);
        assert_eq!(logos[0].service, 0xd210);
        assert_eq!((logos[0].width, logos[0].height), (32, 32));
        assert_eq!(logos[2].url, "d210_128.png");
        assert_eq!(logos[3].service, 0xd220);
        let mut by_rank = logos[..3].to_vec();
        by_rank.sort_by_key(rank);
        assert_eq!(by_rank[0].url, "d210_128.png");
        assert!(service_logos(&[0x03, 0xff]).is_empty());
    }

    #[test]
    fn long_lengths_and_long_service_ids() {
        let big = vec![0x41; 300];
        let encoded = el(0x01, &big);
        let parsed = items(&encoded);
        assert_eq!(parsed[0].1.len(), 300);
        assert_eq!(tlv(&[0x01, 0xff, 0, 0, 2, 9, 9]).unwrap().1, &[9, 9]);
        assert_eq!(
            bearer_sid(&[0x10, 0xe0, 0xd1, 0x10, 0xbc]),
            Some(0xe0d1_10bc)
        );
        assert_eq!(bearer_sid(&[0x00, 0xd2, 0x10]), Some(0xd210));
        assert_eq!(bearer_sid(&[0x40, 0xe0]), None);
    }

    #[test]
    fn the_carousel_hands_out_each_logo_once() {
        let si = si();
        let logo = vec![0x89, b'P', b'N', b'G'];
        let objects = [
            (1u16, mot_header(si.len(), 7, 0, "SI")),
            (2, mot_header(logo.len(), 2, 3, "d210_128.png")),
            (3, mot_header(logo.len(), 2, 3, "d220_32.png")),
        ];
        let mut dir = vec![0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0];
        for (tid, h) in &objects {
            dir.extend_from_slice(&tid.to_be_bytes());
            dir.extend_from_slice(h);
        }
        let mut c = Carousel::new();
        assert!(c.data_group(&group(DG_BODY, 2, 0, true, &logo)).is_empty());
        assert!(
            c.data_group(&group(DG_DIRECTORY, 9, 0, true, &dir))
                .is_empty()
        );
        let first = c.data_group(&group(DG_BODY, 1, 0, true, &si));
        assert_eq!(
            first,
            [Logo {
                service: 0xd210,
                mime: "image/png",
                data: logo.clone()
            }]
        );
        let more = c.data_group(&group(DG_BODY, 3, 0, true, &logo));
        assert_eq!(
            more,
            [Logo {
                service: 0xd220,
                mime: "image/png",
                data: logo.clone()
            }]
        );
        assert!(c.data_group(&group(DG_BODY, 3, 0, true, &logo)).is_empty());
        assert!(c.data_group(&group(0, 3, 0, true, &logo)).is_empty());
        assert!(c.data_group(&[1, 2]).is_empty());
    }
}
