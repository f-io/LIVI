// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use crate::charset;
use crate::fec::crc16_ok;

pub const DG_BODY: u8 = 4;
pub const DG_DIRECTORY: u8 = 6;

#[derive(Debug, PartialEq, Eq)]
pub struct DataGroup<'a> {
    pub kind: u8,
    pub transport: u16,
    pub segment: u16,
    pub last: bool,
    pub data: &'a [u8],
}

pub fn data_group(g: &[u8]) -> Option<DataGroup<'_>> {
    if g.len() < 4 {
        return None;
    }
    let ext = g[0] & 0x80 != 0;
    let has_crc = g[0] & 0x40 != 0;
    let segmented = g[0] & 0x20 != 0;
    let user_access = g[0] & 0x10 != 0;
    if has_crc && !crc16_ok(g) {
        return None;
    }
    let end = if has_crc { g.len() - 2 } else { g.len() };
    let mut i = 2 + if ext { 2 } else { 0 };
    let (mut last, mut segment) = (false, 0u16);
    if segmented {
        let s = g.get(i..i + 2)?;
        last = s[0] & 0x80 != 0;
        segment = (((s[0] & 0x7f) as u16) << 8) | s[1] as u16;
        i += 2;
    }
    let mut transport = None;
    if user_access {
        let ua = *g.get(i)?;
        if ua & 0x10 != 0 {
            let t = g.get(i + 1..i + 3)?;
            transport = Some(u16::from_be_bytes([t[0], t[1]]));
        }
        i += 1 + (ua & 0x0f) as usize;
    }
    let data = g.get(i..end)?;
    if data.len() < 2 {
        return None;
    }
    let size = ((((data[0] & 0x1f) as usize) << 8) | data[1] as usize).min(data.len() - 2);
    Some(DataGroup {
        kind: g[0] & 0x0f,
        transport: transport?,
        segment,
        last,
        data: &data[2..2 + size],
    })
}

#[derive(Default)]
pub struct Segments {
    parts: BTreeMap<u16, Vec<u8>>,
    last: Option<u16>,
}

impl Segments {
    pub fn add(&mut self, number: u16, last: bool, data: &[u8]) {
        self.parts.insert(number, data.to_vec());
        if last {
            self.last = Some(number);
        }
    }

    pub fn complete(&self) -> Option<Vec<u8>> {
        let last = self.last?;
        if (0..=last).all(|n| self.parts.contains_key(&n)) {
            Some(self.parts.values().flatten().copied().collect())
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.parts.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub body_size: usize,
    pub header_size: usize,
    pub content_type: u8,
    pub subtype: u16,
    pub name: Option<String>,
}

impl Header {
    pub fn mime(&self) -> Option<&'static str> {
        match (self.content_type, self.subtype) {
            (2, 1) => Some("image/jpeg"),
            (2, 3) => Some("image/png"),
            _ => None,
        }
    }
}

pub fn header(h: &[u8]) -> Option<Header> {
    if h.len() < 7 {
        return None;
    }
    let body_size = ((h[0] as usize) << 20)
        | ((h[1] as usize) << 12)
        | ((h[2] as usize) << 4)
        | (h[3] as usize >> 4);
    let header_size =
        (((h[3] & 0x0f) as usize) << 9) | ((h[4] as usize) << 1) | (h[5] as usize >> 7);
    let mut out = Header {
        body_size,
        header_size,
        content_type: (h[5] >> 1) & 0x3f,
        subtype: (((h[5] & 1) as u16) << 8) | h[6] as u16,
        name: None,
    };
    let end = if header_size >= 7 {
        header_size.min(h.len())
    } else {
        h.len()
    };
    let mut i = 7;
    while i < end {
        let pli = h[i] >> 6;
        let id = h[i] & 0x3f;
        i += 1;
        let len = match pli {
            0 => 0,
            1 => 1,
            2 => 4,
            _ => {
                let b = *h.get(i)?;
                if b & 0x80 != 0 {
                    let len = (((b & 0x7f) as usize) << 8) | *h.get(i + 1)? as usize;
                    i += 2;
                    len
                } else {
                    i += 1;
                    b as usize
                }
            }
        };
        let value = h.get(i..i + len)?;
        if id == 0x0c && !value.is_empty() {
            out.name = Some(charset::decode(value[0] >> 4, &value[1..]));
        }
        i += len;
    }
    Some(out)
}

pub fn directory(d: &[u8]) -> Option<Vec<(u16, Header)>> {
    if d.len() < 13 || d[0] & 0x80 != 0 {
        return None;
    }
    let objects = u16::from_be_bytes([d[4], d[5]]) as usize;
    let ext_len = u16::from_be_bytes([d[11], d[12]]) as usize;
    let mut i = 13 + ext_len;
    let mut out = Vec::with_capacity(objects);
    for _ in 0..objects {
        let tid = u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]);
        let h = header(d.get(i + 2..)?)?;
        let size = h.header_size.max(7);
        let h = header(d.get(i + 2..i + 2 + size)?)?;
        i += 2 + size;
        out.push((tid, h));
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fec::crc16;

    pub fn with_crc(mut v: Vec<u8>) -> Vec<u8> {
        let c = crc16(&v) ^ 0xffff;
        v.extend_from_slice(&c.to_be_bytes());
        v
    }

    pub fn group(kind: u8, transport: u16, segment: u16, last: bool, data: &[u8]) -> Vec<u8> {
        let seg = (u16::from(last) << 15) | segment;
        let mut g = vec![0x40 | 0x20 | 0x10 | kind, 0x00];
        g.extend_from_slice(&seg.to_be_bytes());
        g.push(0x12);
        g.extend_from_slice(&transport.to_be_bytes());
        g.extend_from_slice(&[(data.len() >> 8) as u8, data.len() as u8]);
        g.extend_from_slice(data);
        with_crc(g)
    }

    pub fn mot_header(body: usize, content_type: u8, subtype: u16, name: &str) -> Vec<u8> {
        let mut ext = vec![0xcc, (name.len() + 1) as u8, 0xf0];
        ext.extend_from_slice(name.as_bytes());
        let size = 7 + ext.len();
        let mut h = vec![
            (body >> 20) as u8,
            (body >> 12) as u8,
            (body >> 4) as u8,
            (((body & 0x0f) << 4) | (size >> 9)) as u8,
            (size >> 1) as u8,
            (((size & 1) << 7) as u8) | (content_type << 1) | (subtype >> 8) as u8,
            subtype as u8,
        ];
        h.extend(ext);
        h
    }

    #[test]
    fn a_data_group_yields_its_segment() {
        let g = group(DG_BODY, 0x1234, 3, true, b"abc");
        let dg = data_group(&g).unwrap();
        assert_eq!(
            dg,
            DataGroup {
                kind: DG_BODY,
                transport: 0x1234,
                segment: 3,
                last: true,
                data: b"abc"
            }
        );
        let mut broken = g.clone();
        broken[9] ^= 1;
        assert!(data_group(&broken).is_none());
        assert!(data_group(&[0, 0]).is_none());
    }

    #[test]
    fn header_core_and_content_name() {
        let h = header(&mot_header(70_000, 2, 3, "logo.png")).unwrap();
        assert_eq!(h.body_size, 70_000);
        assert_eq!(h.header_size, 7 + 3 + 8);
        assert_eq!((h.content_type, h.subtype), (2, 3));
        assert_eq!(h.name.as_deref(), Some("logo.png"));
        assert_eq!(h.mime(), Some("image/png"));
        assert_eq!(header(&mot_header(1, 7, 0, "si")).unwrap().mime(), None);
    }

    #[test]
    fn directory_lists_its_objects() {
        let a = mot_header(100, 7, 0, "SI");
        let b = mot_header(200, 2, 3, "d210.png");
        let mut d = vec![0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 1, 0x00];
        d.extend_from_slice(&5u16.to_be_bytes());
        d.extend(&a);
        d.extend_from_slice(&6u16.to_be_bytes());
        d.extend(&b);
        let list = directory(&d).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, 5);
        assert_eq!(list[1].1.name.as_deref(), Some("d210.png"));
        assert!(
            directory(&[0x80; 13]).is_none(),
            "compressed is not for SPI"
        );
    }

    #[test]
    fn segments_complete_in_order() {
        let mut s = Segments::default();
        assert!(s.is_empty());
        s.add(1, true, b"cd");
        assert!(s.complete().is_none());
        s.add(0, false, b"ab");
        assert_eq!(s.complete().unwrap(), b"abcd");
        assert_eq!(s.len(), 4);
    }
}
