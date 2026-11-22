// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const DIGITS: &[u8; 16] = b"0123456789abcdef";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[hi, lo]| Some(nibble(hi)? << 4 | nibble(lo)?))
        .collect()
}

pub fn rgb(text: &str) -> Option<[u8; 3]> {
    decode(text.trim().trim_start_matches('#'))?.try_into().ok()
}

pub fn mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Reads `38:BA:B0:A0:E6:6F` in the order it is written.
pub fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in &mut out {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

fn nibble(c: u8) -> Option<u8> {
    char::from(c).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mac_reads_in_the_order_it_is_written() {
        assert_eq!(
            parse_mac(" 38:BA:B0:A0:E6:6F\n"),
            Some([0x38, 0xba, 0xb0, 0xa0, 0xe6, 0x6f])
        );
        assert_eq!(parse_mac("38:BA:B0:A0:E6"), None);
        assert_eq!(parse_mac("38:BA:B0:A0:E6:6F:11"), None);
        assert_eq!(parse_mac("38:BA:B0:A0:E6:XX"), None);
        assert_eq!(parse_mac("not an address"), None);
    }

    #[test]
    fn bytes_round_trip_in_lowercase() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(encode(&bytes), "000fa5ff");
        assert_eq!(decode("000fa5ff"), Some(bytes.to_vec()));
        assert_eq!(decode("000FA5FF"), Some(bytes.to_vec()));
        assert_eq!(encode(&[]), "");
        assert_eq!(decode(""), Some(Vec::new()));
    }

    #[test]
    fn rgb_takes_six_digits_with_or_without_hash() {
        assert_eq!(rgb(" #ff8000 "), Some([255, 128, 0]));
        assert_eq!(rgb("00adad"), Some([0, 0xad, 0xad]));
        assert_eq!(rgb("#fff"), None);
        assert_eq!(rgb("nope!!"), None);
    }

    #[test]
    fn mac_is_uppercase_and_colon_separated() {
        assert_eq!(
            mac(&[0x02, 0xab, 0x00, 0x10, 0xff, 0x7c]),
            "02:AB:00:10:FF:7C"
        );
        assert_eq!(mac(&[]), "");
    }

    #[test]
    fn malformed_text_is_nothing() {
        assert_eq!(decode("abc"), None);
        assert_eq!(decode("zz"), None);
        assert_eq!(decode("+f"), None);
        assert_eq!(decode("é0"), None);
    }
}
