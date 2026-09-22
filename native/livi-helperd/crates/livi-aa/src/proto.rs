// The few protobuf primitives the bootstrap and the AV channels need.

pub fn varint(value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut v = value;
    while v > 0x7F {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8 & 0x7F);
    out
}

pub fn pb_string(field: u32, s: &str) -> Vec<u8> {
    let mut out = varint(((field << 3) | 2) as u64);
    out.extend(varint(s.len() as u64));
    out.extend_from_slice(s.as_bytes());
    out
}

// Wire type 0 (varint) is zero, so the tag is just the shifted field number.
pub fn pb_varint(field: u32, value: u64) -> Vec<u8> {
    let mut out = varint((field << 3) as u64);
    out.extend(varint(value));
    out
}

pub fn read_varint(buf: &[u8], i: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*i)?;
        *i += 1;
        val |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(val);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// The first top-level varint field with this number, skipping the others.
pub fn varint_field(buf: &[u8], field: u32) -> Option<u64> {
    let mut i = 0usize;
    while i < buf.len() {
        let tag = read_varint(buf, &mut i)?;
        let value = match tag & 7 {
            0 => Some(read_varint(buf, &mut i)?),
            1 => {
                i += 8;
                None
            }
            2 => {
                let len = read_varint(buf, &mut i)? as usize;
                i += len;
                None
            }
            5 => {
                i += 4;
                None
            }
            _ => return None,
        };
        if (tag >> 3) as u32 == field {
            return value;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips() {
        for v in [0u64, 1, 127, 128, 300, 1 << 40] {
            let mut i = 0;
            assert_eq!(read_varint(&varint(v), &mut i), Some(v));
        }
    }

    #[test]
    fn field_is_found_behind_other_fields() {
        let mut body = pb_string(3, "abc");
        body.extend(pb_varint(1, 7));
        body.extend(pb_varint(2, 9));
        assert_eq!(varint_field(&body, 2), Some(9));
        assert_eq!(varint_field(&body, 1), Some(7));
        assert_eq!(varint_field(&body, 4), None);
    }

    #[test]
    fn garbage_yields_nothing() {
        assert_eq!(varint_field(&[0xFF, 0xFF], 1), None);
    }
}
