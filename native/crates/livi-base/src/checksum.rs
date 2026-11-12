// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const CRC32: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub fn crc32(data: &[u8]) -> u32 {
    !data
        .iter()
        .fold(!0u32, |c, &b| CRC32[usize::from(c as u8 ^ b)] ^ (c >> 8))
}

pub fn inet(parts: &[&[u8]]) -> u16 {
    let mut sum: u32 = parts
        .iter()
        .flat_map(|part| part.chunks(2))
        .map(|p| u32::from(u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inet_is_the_rfc1071_one() {
        let header = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        assert_eq!(inet(&[&header]), 0xb861);
        assert_eq!(inet(&[&header[..10], &header[10..]]), 0xb861);
        assert_eq!(inet(&[&[0x01]]), !0x0100);
    }

    #[test]
    fn crc32_is_the_ieee_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }
}
