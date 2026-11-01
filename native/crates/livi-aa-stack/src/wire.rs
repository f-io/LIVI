// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const WIRE_VARINT: u8 = 0;
pub const WIRE_FIXED64: u8 = 1;
pub const WIRE_LEN: u8 = 2;
pub const WIRE_FIXED32: u8 = 5;

#[cfg(test)]
pub fn encode_varint(value: i64) -> Vec<u8> {
    encode_uvarint(value as u64)
}

#[cfg(test)]
pub fn encode_uvarint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    while v > 0x7f {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
    out
}

#[cfg(test)]
pub fn tag(field: u32, wire: u8) -> Vec<u8> {
    encode_uvarint((u64::from(field) << 3) | u64::from(wire))
}

#[cfg(test)]
pub fn field_varint(field: u32, value: i64) -> Vec<u8> {
    [tag(field, WIRE_VARINT), encode_varint(value)].concat()
}

#[cfg(test)]
pub fn field_len_delim(field: u32, data: &[u8]) -> Vec<u8> {
    [
        tag(field, WIRE_LEN),
        encode_uvarint(data.len() as u64),
        data.to_vec(),
    ]
    .concat()
}

#[cfg(test)]
pub fn field_float(field: u32, value: f64) -> Vec<u8> {
    [
        tag(field, WIRE_FIXED32),
        (value as f32).to_le_bytes().to_vec(),
    ]
    .concat()
}

pub fn read_varint(buf: &[u8], off: usize) -> (u32, usize) {
    let mut result: u32 = 0;
    let mut shift = 0u32;
    let mut pos = off;
    while pos < buf.len() {
        let b = buf[pos];
        result |= u32::from(b & 0x7f).wrapping_shl(shift);
        pos += 1;
        if b & 0x80 == 0 {
            return (result, pos - off);
        }
        shift += 7;
        if shift >= 32 {
            while pos < buf.len() && buf[pos] & 0x80 != 0 {
                pos += 1;
            }
            pos += 1;
            return (result, pos - off);
        }
    }
    (result, pos.saturating_sub(off))
}

pub fn decode_varint_value(bytes: &[u8]) -> u32 {
    read_varint(bytes, 0).0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub session_id: u32,
    pub config_index: Option<u32>,
}

pub fn decode_start(payload: &[u8]) -> Option<Start> {
    let mut off = 0;
    let mut session_id = None;
    let mut config_index = None;
    while off < payload.len() {
        let t = payload[off];
        off += 1;
        let (v, n) = read_varint(payload, off);
        off += n;
        match t {
            0x08 => session_id = Some(v),
            0x10 => config_index = Some(v),
            _ => {}
        }
    }
    session_id.map(|session_id| Start {
        session_id,
        config_index,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<'a> {
    pub field: u32,
    pub wire: u8,
    pub bytes: &'a [u8],
}

pub struct Fields<'a> {
    buf: &'a [u8],
    off: usize,
    done: bool,
}

pub fn decode_fields(buf: &[u8]) -> Fields<'_> {
    Fields {
        buf,
        off: 0,
        done: false,
    }
}

impl<'a> Fields<'a> {
    fn cut(&self, from: usize, len: usize) -> &'a [u8] {
        let start = from.min(self.buf.len());
        let end = from.saturating_add(len).min(self.buf.len());
        &self.buf[start..end]
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Field<'a>;

    fn next(&mut self) -> Option<Field<'a>> {
        if self.done || self.off >= self.buf.len() {
            return None;
        }
        let (tag, tn) = read_varint(self.buf, self.off);
        self.off += tn;
        let wire = (tag & 7) as u8;
        let field = tag >> 3;
        let (from, len) = match wire {
            WIRE_VARINT => {
                let (_, vn) = read_varint(self.buf, self.off);
                (self.off, vn)
            }
            WIRE_FIXED64 => (self.off, 8),
            WIRE_LEN => {
                let (len, ln) = read_varint(self.buf, self.off);
                (self.off + ln, len as usize)
            }
            WIRE_FIXED32 => (self.off, 4),
            _ => {
                self.done = true;
                return None;
            }
        };
        let bytes = self.cut(from, len);
        self.off = from.saturating_add(len);
        Some(Field { field, wire, bytes })
    }
}

pub fn has_field(buf: &[u8], field: u32) -> bool {
    decode_fields(buf).any(|f| f.field == field)
}

pub fn round_half_up(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_wrap_negatives_to_ten_bytes() {
        assert_eq!(encode_varint(0), [0]);
        assert_eq!(encode_varint(300), [0xac, 0x02]);
        assert_eq!(
            encode_varint(-1),
            [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
        );
        assert_eq!(field_varint(2, 1), [0x10, 0x01]);
        assert_eq!(field_len_delim(1, &[7]), [0x0a, 0x01, 0x07]);
        assert_eq!(field_float(1, 0.36), [0x0d, 0xec, 0x51, 0xb8, 0x3e]);
    }

    #[test]
    fn long_varints_keep_their_low_bits() {
        assert_eq!(read_varint(&[0xac, 0x02], 0), (300, 2));
        let long = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x05];
        assert_eq!(read_varint(&long, 0), (u32::MAX, 7));
        assert_eq!(read_varint(&[0x80], 0), (0, 1));
        assert_eq!(read_varint(&[0x80, 0x80, 0x80, 0x80, 0x80], 0), (0, 6));
        assert_eq!(read_varint(&[], 0), (0, 0));
    }

    #[test]
    fn start_needs_a_session_id() {
        assert_eq!(
            decode_start(&[0x08, 0x05, 0x10, 0x01]),
            Some(Start {
                session_id: 5,
                config_index: Some(1)
            })
        );
        assert_eq!(
            decode_start(&[0x18, 0x02, 0x08, 0x00]).map(|s| s.config_index),
            Some(None)
        );
        assert_eq!(decode_start(&[0x10, 0x01]), None);
        assert_eq!(
            decode_start(&[0x08]),
            Some(Start {
                session_id: 0,
                config_index: None
            })
        );
    }

    #[test]
    fn fields_are_walked_and_cut_at_the_end() {
        let msg = [
            0x08, 0x96, 0x01, 0x12, 0x02, b'h', b'i', 0x1d, 1, 2, 3, 4, 0x21, 1, 2,
        ];
        let fields: Vec<_> = decode_fields(&msg).collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(
            fields[0],
            Field {
                field: 1,
                wire: 0,
                bytes: &[0x96, 0x01]
            }
        );
        assert_eq!(fields[1].bytes, b"hi");
        assert_eq!(fields[2].bytes, &[1, 2, 3, 4]);
        assert_eq!(fields[3].bytes, &[1, 2]);
        assert_eq!(decode_fields(&[0x0b, 0x08, 0x01]).count(), 0);
        assert_eq!(
            decode_fields(&[0x0a, 0x7f, 0x01]).next().unwrap().bytes,
            &[0x01]
        );
        assert!(has_field(&msg, 3));
        assert!(!has_field(&msg, 5));
    }

    #[test]
    fn halves_round_up() {
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(-2.5), -2.0);
        assert_eq!(round_half_up(-2.51), -3.0);
        assert_eq!(round_half_up(0.49999999999999994), 0.0);
    }
}
