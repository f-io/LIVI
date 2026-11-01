// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    Data(Vec<u8>),
    Uuid([u8; 16]),
    Array(Vec<Value>),
    Dict(Vec<(Value, Value)>),
}

impl Value {
    pub fn str(s: &str) -> Self {
        Value::String(s.into())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Value::String(s) if s == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_data(&self) -> Option<&[u8]> {
        match self {
            Value::Data(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Dict(entries) => Some(entries),
            _ => None,
        }
    }
}

pub fn dict<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Dict(
        entries
            .into_iter()
            .map(|(k, v)| (Value::str(k), v))
            .collect(),
    )
}

const TRUE: u8 = 0x01;
const FALSE: u8 = 0x02;
const END: u8 = 0x03;
const NULL: u8 = 0x04;
const UUID: u8 = 0x05;
const DATE: u8 = 0x06;
const SMALL_INT: u8 = 0x08;
const SMALL_INT_MAX: i64 = 39;
const INT8: u8 = 0x30;
const FLOAT32: u8 = 0x35;
const FLOAT64: u8 = 0x36;
const STRING: u8 = 0x40;
const DATA: u8 = 0x70;
const SHORT_LEN: usize = 0x20;
const REF: u8 = 0xA0;
const REF_WIDE: u8 = 0xC0;
const ARRAY: u8 = 0xD0;
const DICT: u8 = 0xE0;
const SHORT_COUNT: usize = 14;
const OPEN: u8 = 0x0F;

pub fn decode(buf: &[u8]) -> Option<Value> {
    let mut reader = Reader {
        buf,
        at: 0,
        seen: Vec::new(),
    };
    let v = reader.value()?;
    (reader.at == buf.len()).then_some(v)
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
    seen: Vec<Value>,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.buf.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn unsigned(&mut self, width: usize) -> Option<u64> {
        Some(
            self.take(width)?
                .iter()
                .rev()
                .fold(0, |acc, b| (acc << 8) | u64::from(*b)),
        )
    }

    fn signed(&mut self, width: usize) -> Option<i64> {
        let raw = self.unsigned(width)?;
        let shift = 64 - 8 * width as u32;
        Some(((raw << shift) as i64) >> shift)
    }

    fn len(&mut self, tag: u8, base: u8) -> Option<usize> {
        let n = usize::from(tag - base);
        if n <= SHORT_LEN {
            return Some(n);
        }
        usize::try_from(self.unsigned(n - SHORT_LEN)?).ok()
    }

    fn remember(&mut self, v: Value) -> Value {
        self.seen.push(v.clone());
        v
    }

    fn value(&mut self) -> Option<Value> {
        let tag = *self.take(1)?.first()?;
        Some(match tag {
            TRUE => Value::Bool(true),
            FALSE => Value::Bool(false),
            NULL => Value::Null,
            UUID => {
                let v = Value::Uuid(self.take(16)?.try_into().ok()?);
                self.remember(v)
            }
            DATE | FLOAT64 => {
                let v = Value::Real(f64::from_bits(self.unsigned(8)?));
                self.remember(v)
            }
            FLOAT32 => {
                let v = Value::Real(f64::from(f32::from_bits(self.unsigned(4)? as u32)));
                self.remember(v)
            }
            SMALL_INT..INT8 => Value::Int(i64::from(tag - SMALL_INT)),
            0x30..=0x33 => {
                let v = Value::Int(self.signed(1 << (tag - INT8))?);
                self.remember(v)
            }
            0x40..=0x64 => {
                let n = self.len(tag, STRING)?;
                let v = Value::String(String::from_utf8_lossy(self.take(n)?).into_owned());
                self.remember(v)
            }
            0x70..=0x94 => {
                let n = self.len(tag, DATA)?;
                let v = Value::Data(self.take(n)?.to_vec());
                self.remember(v)
            }
            REF..=REF_WIDE => self.seen.get(usize::from(tag - REF))?.clone(),
            0xC1..=0xC4 => {
                let index = usize::try_from(self.unsigned(usize::from(tag - REF_WIDE))?).ok()?;
                self.seen.get(index)?.clone()
            }
            0xD0..=0xDF => {
                let count = (tag - ARRAY != OPEN).then_some(usize::from(tag - ARRAY));
                let mut items = Vec::new();
                while count.is_none_or(|n| items.len() < n) {
                    if count.is_none() && self.buf.get(self.at) == Some(&END) {
                        self.at += 1;
                        break;
                    }
                    items.push(self.value()?);
                }
                Value::Array(items)
            }
            0xE0..=0xEF => {
                let count = (tag - DICT != OPEN).then_some(usize::from(tag - DICT));
                let mut entries = Vec::new();
                while count.is_none_or(|n| entries.len() < n) {
                    if count.is_none() && self.buf.get(self.at) == Some(&END) {
                        self.at += 1;
                        break;
                    }
                    let key = self.value()?;
                    entries.push((key, self.value()?));
                }
                Value::Dict(entries)
            }
            _ => return None,
        })
    }
}

pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write(&mut out, v);
    out
}

fn write_len(out: &mut Vec<u8>, base: u8, n: usize) {
    if n <= SHORT_LEN {
        out.push(base + n as u8);
        return;
    }
    let width = match n {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    out.push(base + SHORT_LEN as u8 + width as u8);
    out.extend_from_slice(&(n as u64).to_le_bytes()[..width]);
}

fn write_count(out: &mut Vec<u8>, base: u8, n: usize) -> bool {
    if n <= SHORT_COUNT {
        out.push(base + n as u8);
        false
    } else {
        out.push(base + OPEN);
        true
    }
}

fn write(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null => out.push(NULL),
        Value::Bool(b) => out.push(if *b { TRUE } else { FALSE }),
        Value::Int(n @ 0..=SMALL_INT_MAX) => out.push(SMALL_INT + *n as u8),
        Value::Int(n) => {
            let width: usize = if i8::try_from(*n).is_ok() {
                1
            } else if i16::try_from(*n).is_ok() {
                2
            } else if i32::try_from(*n).is_ok() {
                4
            } else {
                8
            };
            out.push(INT8 + width.trailing_zeros() as u8);
            out.extend_from_slice(&n.to_le_bytes()[..width]);
        }
        Value::Real(f) => {
            out.push(FLOAT64);
            out.extend_from_slice(&f.to_le_bytes());
        }
        Value::String(s) => {
            write_len(out, STRING, s.len());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Data(d) => {
            write_len(out, DATA, d.len());
            out.extend_from_slice(d);
        }
        Value::Uuid(u) => {
            out.push(UUID);
            out.extend_from_slice(u);
        }
        Value::Array(items) => {
            let open = write_count(out, ARRAY, items.len());
            for item in items {
                write(out, item);
            }
            if open {
                out.push(END);
            }
        }
        Value::Dict(entries) => {
            let open = write_count(out, DICT, entries.len());
            for (k, v) in entries {
                write(out, k);
                write(out, v);
            }
            if open {
                out.push(END);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_come_back_as_they_went() {
        let long = "x".repeat(300);
        let v = dict([
            ("small", Value::Int(39)),
            ("byte", Value::Int(-1)),
            ("short", Value::Int(273)),
            ("error", Value::Int(-6727)),
            ("word", Value::Int(0x4500_010C)),
            ("wide", Value::Int(4_294_967_295)),
            ("real", Value::Real(1.5)),
            ("text", Value::str(&long)),
            ("blob", Value::Data(vec![7; 40])),
            ("id", Value::Uuid([3; 16])),
            ("none", Value::Null),
            ("many", Value::Array((0..20).map(Value::Int).collect())),
            (
                "keys",
                Value::Dict(vec![
                    (Value::Int(262), Value::Bool(true)),
                    (Value::Int(0), Value::Bool(false)),
                ]),
            ),
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
            ("c", Value::Int(3)),
        ]);
        let bytes = encode(&v);
        assert_eq!(
            bytes[0],
            DICT + OPEN,
            "more than {SHORT_COUNT} entries end with END"
        );
        assert_eq!(decode(&bytes), Some(v));
    }

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn a_register_answer_with_references_reads_whole() {
        let inner = hex(
            "e4466572726f7273eb31040131b9e5310d01a2310501a2310e01a2310a01a2310601a2310201a2
             310f01a2310b01a2310301a2310c01a24d7472616e73616374696f6e494433a0bc52cb03e5cd02
             47636f6d6d616e6450726567697374657252657370 6f6e7365467661 6c756573e23108010131110108",
        );
        let v = decode(&inner).unwrap();
        assert_eq!(
            v.get("command").and_then(Value::as_str),
            Some("registerResponse")
        );
        let errors = v.get("errors").and_then(Value::as_dict).unwrap();
        assert_eq!(errors.len(), 11);
        assert!(errors.iter().all(|(_, e)| *e == Value::Int(-6727)));
        assert_eq!(errors[1].0, Value::Int(269));
        assert_eq!(
            v.get("values"),
            Some(&Value::Dict(vec![
                (Value::Int(264), Value::Bool(true)),
                (Value::Int(273), Value::Int(0)),
            ]))
        );
        assert!(v.get("transactionID").and_then(Value::as_int).is_some());
    }

    #[test]
    fn numbers_take_the_smallest_signed_width() {
        assert_eq!(encode(&Value::Int(5)), [0x0D]);
        assert_eq!(encode(&Value::Int(255)), [0x31, 0xFF, 0x00]);
        assert_eq!(encode(&Value::Int(-6727)), [0x31, 0xB9, 0xE5]);
        assert_eq!(encode(&Value::Int(65535)), [0x32, 0xFF, 0xFF, 0x00, 0x00]);
        assert_eq!(
            encode(&Value::Int(4_294_967_295)),
            [0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn back_references_point_at_strings_and_wide_numbers() {
        let mut msg = vec![0xE3, 0x43];
        msg.extend_from_slice(b"iid");
        msg.extend_from_slice(&[0x31, 0x11, 0x01, 0x44]);
        msg.extend_from_slice(b"type");
        msg.push(0xA1);
        msg.push(0x44);
        msg.extend_from_slice(b"list");
        msg.extend_from_slice(&[0xD3, 0xA0, 0x0D, 0xC1, 0x02]);
        let v = decode(&msg).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_int), Some(273));
        assert_eq!(
            v.get("list").and_then(Value::as_array),
            Some(&[Value::str("iid"), Value::Int(5), Value::str("type")][..])
        );
        assert_eq!(decode(&[0xA0]), None);
        assert_eq!(decode(&[0xE1, 0x41, b'k']), None);
    }
}
