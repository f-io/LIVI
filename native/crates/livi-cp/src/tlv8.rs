// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

pub fn encode(items: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Option<u8> = None;
    for &(kind, value) in items {
        if prev == Some(kind) {
            out.extend_from_slice(&[0xff, 0x00]);
        }
        let mut off = 0;
        loop {
            let len = (value.len() - off).min(255);
            out.push(kind);
            out.push(len as u8);
            out.extend_from_slice(&value[off..off + len]);
            off += len;
            if off >= value.len() {
                break;
            }
        }
        prev = Some(kind);
    }
    out
}

pub fn decode(buf: &[u8]) -> BTreeMap<u8, Vec<u8>> {
    let mut out: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    let mut p = 0;
    let mut last: Option<(u8, usize)> = None;
    while p + 2 <= buf.len() {
        let kind = buf[p];
        let len = usize::from(buf[p + 1]);
        let value = &buf[p + 2..(p + 2 + len).min(buf.len())];
        p += 2 + len;
        if last == Some((kind, 255)) {
            out.entry(kind).or_default().extend_from_slice(value);
        } else {
            out.insert(kind, value.to_vec());
        }
        last = Some((kind, len));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_fragments() {
        let long = vec![7u8; 600];
        let buf = encode(&[(6, &[1]), (3, &long), (1, b"id")]);
        assert_eq!(&buf[..3], &[6, 1, 1]);
        assert_eq!(&buf[3..5], &[3, 255]);
        let map = decode(&buf);
        assert_eq!(map[&6], vec![1]);
        assert_eq!(map[&3], long);
        assert_eq!(map[&1], b"id".to_vec());
    }

    #[test]
    fn adjacent_items_of_one_type_get_a_separator() {
        assert_eq!(
            encode(&[(1, b"a"), (1, b"b")]),
            vec![1, 1, b'a', 0xff, 0, 1, 1, b'b']
        );
        assert_eq!(decode(&encode(&[(1, b"a"), (1, b"b")]))[&1], b"b".to_vec());
    }

    #[test]
    fn an_empty_value_is_one_empty_item() {
        assert_eq!(encode(&[(5, &[])]), vec![5, 0]);
    }

    #[test]
    fn a_truncated_item_keeps_what_arrived() {
        assert_eq!(decode(&[3, 4, 9, 9])[&3], vec![9, 9]);
        assert!(decode(&[3]).is_empty());
    }
}
