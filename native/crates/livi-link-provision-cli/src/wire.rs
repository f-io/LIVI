// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const MAGIC: u32 = 0x55aa_55aa;
const HEADER_LEN: usize = 16;

fn header(kind: u32, len: usize) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(&MAGIC.to_le_bytes());
    h[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    h[8..12].copy_from_slice(&kind.to_le_bytes());
    h[12..].copy_from_slice(&(!kind).to_le_bytes());
    h
}

pub fn message(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header(kind, payload.len()));
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_matches_the_stock_layout() {
        let h = header(0xaa, 0);
        assert_eq!(&h[..4], &[0xaa, 0x55, 0xaa, 0x55]);
        assert_eq!(&h[4..8], &[0, 0, 0, 0]);
        assert_eq!(&h[8..12], &[0xaa, 0, 0, 0]);
        assert_eq!(&h[12..], &[0x55, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn a_message_is_the_header_and_the_payload() {
        let wire = message(0x99, &[1, 2, 3]);
        assert_eq!(&wire[..HEADER_LEN], &header(0x99, 3));
        assert_eq!(&wire[HEADER_LEN..], &[1, 2, 3]);
    }
}
