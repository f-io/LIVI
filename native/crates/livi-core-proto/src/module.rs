// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The UART module's frames, `02 dir cmd len data.. xor` with the xor over dir to the last data byte.
//! core passes them unchanged between the module and the helper's socket.

pub const HOST: u8 = 0x80;
pub const CHIP: u8 = 0x81;

pub fn frame(dir: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x02, dir, cmd, data.len() as u8];
    f.extend_from_slice(data);
    let xor = f[1..].iter().fold(0, |x, b| x ^ b);
    f.push(xor);
    f
}

/// Takes the first whole frame off `buf`, dropping bytes that do not start one.
pub fn next_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let start = buf.iter().position(|&b| b == 0x02).unwrap_or(buf.len());
        buf.drain(..start);
        if buf.len() < 5 || buf.len() < 5 + usize::from(buf[3]) {
            return None;
        }
        let len = 5 + usize::from(buf[3]);
        if buf[1..len - 1].iter().fold(0, |x, b| x ^ b) == buf[len - 1] {
            return Some(buf.drain(..len).collect());
        }
        buf.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_ends_in_the_xor_from_dir_on() {
        assert_eq!(frame(HOST, 0xa4, &[]), [0x02, 0x80, 0xa4, 0x00, 0x24]);
        assert_eq!(
            frame(HOST, 0xb0, &[0xce, 0x01, 0, 0, 0]),
            [0x02, 0x80, 0xb0, 0x05, 0xce, 0x01, 0x00, 0x00, 0x00, 0xfa]
        );
    }

    #[test]
    fn noise_and_a_bad_xor_are_skipped() {
        let good = frame(CHIP, 0xd4, b"AD");
        let mut buf = vec![0xff, 0x02, 0x81, 0xd4, 0x00, 0x00];
        buf.extend_from_slice(&good);
        assert_eq!(next_frame(&mut buf), Some(good));
        assert!(buf.is_empty());
    }
}
