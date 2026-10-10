//! Only the bplist00 subset the control channel carries.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(u64),
    Real(f64),
    String(String),
    Data(Vec<u8>),
    Array(Vec<Value>),
    /// Keys in the order they are written.
    Dict(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<u64> {
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
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

pub fn dict<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Dict(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[derive(Debug, PartialEq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bplist: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

const MAGIC: &[u8] = b"bplist00";
/// A plist whose references loop back would recurse without end.
const MAX_DEPTH: usize = 64;

fn err<T>(msg: impl Into<String>) -> Result<T, DecodeError> {
    Err(DecodeError(msg.into()))
}

struct Reader<'a> {
    buf: &'a [u8],
    offsets: Vec<usize>,
    ref_size: usize,
}

impl Reader<'_> {
    fn sized(&self, at: usize, size: usize) -> Result<u128, DecodeError> {
        let bytes =
            self.buf.get(at..at + size).ok_or_else(|| DecodeError("out of bounds".into()))?;
        Ok(bytes.iter().fold(0u128, |v, &b| (v << 8) | u128::from(b)))
    }

    fn usize_at(&self, at: usize, size: usize) -> Result<usize, DecodeError> {
        usize::try_from(self.sized(at, size)?).or_else(|_| err("offset too large"))
    }

    fn byte(&self, at: usize) -> Result<u8, DecodeError> {
        self.buf.get(at).copied().ok_or_else(|| DecodeError("out of bounds".into()))
    }

    fn slice(&self, at: usize, len: usize) -> Result<&[u8], DecodeError> {
        self.buf.get(at..at.saturating_add(len)).ok_or_else(|| DecodeError("out of bounds".into()))
    }

    fn object(&self, index: usize, depth: usize) -> Result<Value, DecodeError> {
        if depth > MAX_DEPTH {
            return err("nested too deep");
        }
        let mut p =
            *self.offsets.get(index).ok_or_else(|| DecodeError("bad object reference".into()))?;
        let marker = self.byte(p)?;
        let (kind, nib) = (marker >> 4, marker & 0x0f);
        p += 1;

        let mut count = || -> Result<usize, DecodeError> {
            if nib != 0x0f {
                return Ok(usize::from(nib));
            }
            let size = 1usize << (self.byte(p)? & 0x0f);
            let c = self.usize_at(p + 1, size)?;
            p += 1 + size;
            Ok(c)
        };

        match kind {
            0x0 => match nib {
                0x08 => Ok(Value::Bool(false)),
                0x09 => Ok(Value::Bool(true)),
                _ => err(format!("unsupported primitive 0x0{nib:x}")),
            },
            0x1 => {
                let v = self.sized(p, 1 << nib)?;
                u64::try_from(v).map(Value::Int).or_else(|_| err("integer too large"))
            }
            0x2 => {
                let size = 1usize << nib;
                let bytes = self.slice(p, size)?;
                match size {
                    4 => Ok(Value::Real(f64::from(f32::from_be_bytes(bytes.try_into().unwrap())))),
                    8 => Ok(Value::Real(f64::from_be_bytes(bytes.try_into().unwrap()))),
                    _ => err("unsupported real size"),
                }
            }
            // Seconds since 2001, read as a plain number.
            0x3 if nib == 3 => {
                Ok(Value::Real(f64::from_be_bytes(self.slice(p, 8)?.try_into().unwrap())))
            }
            0x4 => {
                let n = count()?;
                Ok(Value::Data(self.slice(p, n)?.to_vec()))
            }
            0x5 => {
                let n = count()?;
                Ok(Value::String(self.slice(p, n)?.iter().map(|&b| char::from(b & 0x7f)).collect()))
            }
            0x6 => {
                let n = count()?;
                let units: Vec<u16> = self
                    .slice(p, n * 2)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                Ok(Value::String(String::from_utf16_lossy(&units)))
            }
            0xa => {
                let n = count()?;
                let mut items = Vec::with_capacity(n.min(1024));
                for i in 0..n {
                    let r = self.usize_at(p + i * self.ref_size, self.ref_size)?;
                    items.push(self.object(r, depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            0xd => {
                let n = count()?;
                let mut entries = Vec::with_capacity(n.min(1024));
                for i in 0..n {
                    let kr = self.usize_at(p + i * self.ref_size, self.ref_size)?;
                    let vr = self.usize_at(p + (n + i) * self.ref_size, self.ref_size)?;
                    let key = match self.object(kr, depth + 1)? {
                        Value::String(s) => s,
                        Value::Int(n) => n.to_string(),
                        Value::Real(r) => r.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => String::new(),
                    };
                    entries.push((key, self.object(vr, depth + 1)?));
                }
                Ok(Value::Dict(entries))
            }
            _ => err(format!("unsupported object type 0x{kind:x}")),
        }
    }
}

pub fn decode(buf: &[u8]) -> Result<Value, DecodeError> {
    if buf.len() < MAGIC.len() + 32 || &buf[..MAGIC.len()] != MAGIC {
        return err("bad magic or too short");
    }
    let trailer = &buf[buf.len() - 32..];
    let offset_size = usize::from(trailer[6]);
    let ref_size = usize::from(trailer[7]);
    let be64 = |at: usize| u64::from_be_bytes(trailer[at..at + 8].try_into().unwrap());
    let (num_objects, top, table) = (be64(8), be64(16), be64(24));

    let mut reader = Reader { buf, offsets: Vec::new(), ref_size };
    let table = usize::try_from(table).or_else(|_| err("offset table out of range"))?;
    let num_objects = usize::try_from(num_objects).or_else(|_| err("too many objects"))?;
    if num_objects.saturating_mul(offset_size) > buf.len() {
        return err("offset table out of range");
    }
    for i in 0..num_objects {
        let off = reader.usize_at(table + i * offset_size, offset_size)?;
        reader.offsets.push(off);
    }
    let top = usize::try_from(top).or_else(|_| err("bad top object"))?;
    reader.object(top, 0)
}

enum Node {
    Leaf(Vec<u8>),
    Container { head: Vec<u8>, refs: Vec<usize> },
}

fn be(value: u64, size: usize) -> Vec<u8> {
    value.to_be_bytes()[8 - size..].to_vec()
}

fn marker(kind: u8, count: usize) -> Vec<u8> {
    if count < 0x0f {
        return vec![(kind << 4) | count as u8];
    }
    let (size, log) = if count > 0xffff {
        (4, 2)
    } else if count > 0xff {
        (2, 1)
    } else {
        (1, 0)
    };
    let mut out = vec![(kind << 4) | 0x0f, 0x10 | log];
    out.extend(be(count as u64, size));
    out
}

fn int_bytes(n: u64) -> Vec<u8> {
    let (size, log) = if n > 0xffff_ffff {
        (8, 3)
    } else if n > 0xffff {
        (4, 2)
    } else if n > 0xff {
        (2, 1)
    } else {
        (1, 0)
    };
    let mut out = vec![0x10 | log];
    out.extend(be(n, size));
    out
}

fn add(nodes: &mut Vec<Node>, value: &Value) -> usize {
    let idx = nodes.len();
    nodes.push(Node::Leaf(Vec::new()));
    let node = match value {
        Value::Bool(b) => Node::Leaf(vec![if *b { 0x09 } else { 0x08 }]),
        Value::Int(n) => Node::Leaf(int_bytes(*n)),
        Value::Real(r) => {
            let mut out = vec![0x23];
            out.extend_from_slice(&r.to_be_bytes());
            Node::Leaf(out)
        }
        Value::String(s) if s.is_ascii() => {
            let mut out = marker(0x5, s.len());
            out.extend_from_slice(s.as_bytes());
            Node::Leaf(out)
        }
        Value::String(s) => {
            let units: Vec<u16> = s.encode_utf16().collect();
            let mut out = marker(0x6, units.len());
            out.extend(units.iter().flat_map(|u| u.to_be_bytes()));
            Node::Leaf(out)
        }
        Value::Data(d) => {
            let mut out = marker(0x4, d.len());
            out.extend_from_slice(d);
            Node::Leaf(out)
        }
        Value::Array(items) => {
            let refs = items.iter().map(|v| add(nodes, v)).collect::<Vec<_>>();
            Node::Container { head: marker(0xa, refs.len()), refs }
        }
        Value::Dict(entries) => {
            let mut refs: Vec<usize> =
                entries.iter().map(|(k, _)| add(nodes, &Value::String(k.clone()))).collect();
            refs.extend(entries.iter().map(|(_, v)| add(nodes, v)).collect::<Vec<_>>());
            Node::Container { head: marker(0xd, entries.len()), refs }
        }
    };
    nodes[idx] = node;
    idx
}

pub fn encode(root: &Value) -> Vec<u8> {
    let mut nodes = Vec::new();
    let top = add(&mut nodes, root);
    let ref_size = if nodes.len() > 0xffff {
        4
    } else if nodes.len() > 0xff {
        2
    } else {
        1
    };

    let mut out = MAGIC.to_vec();
    let mut offsets = Vec::with_capacity(nodes.len());
    for node in &nodes {
        offsets.push(out.len());
        match node {
            Node::Leaf(body) => out.extend_from_slice(body),
            Node::Container { head, refs } => {
                out.extend_from_slice(head);
                for &r in refs {
                    out.extend(be(r as u64, ref_size));
                }
            }
        }
    }

    let table = out.len();
    let offset_size = if table > 0xffff {
        4
    } else if table > 0xff {
        2
    } else {
        1
    };
    for off in offsets {
        out.extend(be(off as u64, offset_size));
    }
    let mut trailer = [0u8; 32];
    trailer[6] = offset_size as u8;
    trailer[7] = ref_size as u8;
    trailer[8..16].copy_from_slice(&(nodes.len() as u64).to_be_bytes());
    trailer[16..24].copy_from_slice(&(top as u64).to_be_bytes());
    trailer[24..32].copy_from_slice(&(table as u64).to_be_bytes());
    out.extend_from_slice(&trailer);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_types_and_key_order() {
        let value = dict([
            ("type", Value::String("hidSendReport".into())),
            ("uuid", Value::String("2a2a2a2a".into())),
            ("hidReport", Value::Data(vec![0, 1, 2])),
            ("big", Value::Int(0x1_0000_0000)),
            ("ratio", Value::Real(-1.5)),
            ("on", Value::Bool(true)),
            ("name", Value::String("Bülli".into())),
            ("list", Value::Array(vec![Value::Int(1), Value::Int(300), Value::Int(70000)])),
        ]);
        assert_eq!(decode(&encode(&value)), Ok(value));
    }

    #[test]
    fn a_date_reads_as_its_seconds() {
        let mut bytes = encode(&Value::Real(813_000_000.5));
        assert_eq!(bytes[8], 0x23);
        bytes[8] = 0x33;
        assert_eq!(decode(&bytes), Ok(Value::Real(813_000_000.5)));
        bytes[8] = 0x32;
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn long_containers_use_a_count_object() {
        let long = Value::Array((0..300).map(Value::Int).collect());
        let bytes = encode(&long);
        assert_eq!(decode(&bytes), Ok(long));
        assert_eq!(bytes[8], 0xaf);
    }

    #[test]
    fn rejects_garbage_and_loops() {
        assert!(decode(b"nope").is_err());
        assert!(decode(&[b"bplist00".as_slice(), &[0u8; 32]].concat()).is_err());
        // One array that contains itself.
        let mut looped = b"bplist00".to_vec();
        looped.extend_from_slice(&[0xa1, 0x00]);
        let table = looped.len();
        looped.push(8);
        let mut trailer = [0u8; 32];
        trailer[6] = 1;
        trailer[7] = 1;
        trailer[15] = 1;
        trailer[31] = table as u8;
        looped.extend_from_slice(&trailer);
        assert_eq!(decode(&looped), Err(DecodeError("nested too deep".into())));
    }

    #[test]
    fn accessors_read_dict_entries() {
        let v = dict([("type", Value::Int(110)), ("on", Value::Bool(false))]);
        assert_eq!(v.get("type").and_then(Value::as_int), Some(110));
        assert_eq!(v.get("on").and_then(Value::as_bool), Some(false));
        assert_eq!(v.get("missing"), None);
        assert_eq!(Value::Int(1).get("x"), None);
    }
}
