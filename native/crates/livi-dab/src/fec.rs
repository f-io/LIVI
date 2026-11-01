// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const POLYS: [u8; 4] = [0o133, 0o171, 0o145, 0o133];
const STATES: usize = 64;
pub const TAIL_BITS: usize = 6;

fn pi(n: usize) -> [bool; 32] {
    const ORDER: [usize; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    let mut ones = [1usize; 8];
    for k in 0..n {
        ones[ORDER[k % 8]] += 1;
    }
    let mut v = [false; 32];
    for (g, &count) in ones.iter().enumerate() {
        for b in 0..count {
            v[g * 4 + b] = true;
        }
    }
    v
}

const TAIL: [bool; 24] = [
    true, true, false, false, true, true, false, false, true, true, false, false, true, true,
    false, false, true, true, false, false, true, true, false, false,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Puncturing {
    pub runs: Vec<(usize, usize)>,
}

impl Puncturing {
    pub fn info_bits(&self) -> usize {
        self.runs.iter().map(|r| r.0 * 32).sum()
    }

    pub fn coded_bits(&self) -> usize {
        self.runs
            .iter()
            .map(|&(blocks, n)| blocks * 4 * (8 + n))
            .sum::<usize>()
            + 12
    }

    pub fn depuncture(&self, input: &[i8]) -> Vec<i8> {
        let mut out = Vec::with_capacity(4 * (self.info_bits() + TAIL_BITS));
        let mut it = input.iter().copied();
        for &(blocks, n) in &self.runs {
            let v = pi(n);
            for _ in 0..blocks * 4 {
                for &keep in &v {
                    out.push(if keep { it.next().unwrap_or(0) } else { 0 });
                }
            }
        }
        for &keep in &TAIL {
            out.push(if keep { it.next().unwrap_or(0) } else { 0 });
        }
        out
    }
}

fn parity(x: u8) -> u8 {
    (x.count_ones() & 1) as u8
}

pub fn viterbi(soft: &[i8]) -> Vec<u8> {
    let steps = soft.len() / 4;
    let mut outputs = [[0u8; 4]; 128];
    for (reg, o) in outputs.iter_mut().enumerate() {
        for (j, &p) in POLYS.iter().enumerate() {
            o[j] = parity(reg as u8 & p);
        }
    }
    let mut metric = [i32::MIN / 2; STATES];
    metric[0] = 0;
    let mut decisions = vec![0u64; steps];
    for (t, chunk) in soft.as_chunks::<4>().0.iter().enumerate() {
        let mut next = [i32::MIN / 2; STATES];
        let mut dec = 0u64;
        for (ns, slot) in next.iter_mut().enumerate() {
            let input = ns >> 5;
            let mut best = i32::MIN;
            let mut pick = 0;
            for x in 0..2 {
                let prev = ((ns & 0x1f) << 1) | x;
                let reg = (input << 6) | prev;
                let o = &outputs[reg];
                let mut m = metric[prev];
                for j in 0..4 {
                    let s = chunk[j] as i32;
                    m += if o[j] == 0 { s } else { -s };
                }
                if m > best {
                    best = m;
                    pick = x;
                }
            }
            *slot = best;
            dec |= (pick as u64) << ns;
        }
        decisions[t] = dec;
        metric = next;
    }
    let mut state = 0usize;
    let mut bits = vec![0u8; steps];
    for t in (0..steps).rev() {
        bits[t] = (state >> 5) as u8;
        let x = ((decisions[t] >> state) & 1) as usize;
        state = ((state & 0x1f) << 1) | x;
    }
    bits.truncate(steps.saturating_sub(TAIL_BITS));
    bits
}

pub fn descramble(bits: &mut [u8]) {
    let mut reg: u16 = 0x1ff;
    for b in bits.iter_mut() {
        let p = ((reg >> 8) ^ (reg >> 4)) as u8 & 1;
        reg = ((reg << 1) | p as u16) & 0x1ff;
        *b ^= p;
    }
}

pub fn pack(bits: &[u8]) -> Vec<u8> {
    bits.chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b))
        .collect()
}

pub fn crc16_ok(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    let (body, crc) = data.split_at(data.len() - 2);
    crc16(body) ^ 0xffff == u16::from_be_bytes([crc[0], crc[1]])
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
pub(crate) fn encode(info: &[u8]) -> Vec<u8> {
    let mut reg = 0u8;
    let mut out = Vec::new();
    for &b in info.iter().chain([0u8; TAIL_BITS].iter()) {
        let r = (b << 6) | reg;
        for p in POLYS {
            out.push(parity(r & p));
        }
        reg = r >> 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn punct(coded: &[u8], p: &Puncturing) -> Vec<i8> {
        let mut out = Vec::new();
        let mut i = 0;
        for &(blocks, n) in &p.runs {
            let v = pi(n);
            for _ in 0..blocks * 4 {
                for &keep in &v {
                    if keep {
                        out.push(if coded[i] == 0 { 60 } else { -60 });
                    }
                    i += 1;
                }
            }
        }
        for &keep in &TAIL {
            if keep {
                out.push(if coded[i] == 0 { 60 } else { -60 });
            }
            i += 1;
        }
        out
    }

    #[test]
    fn puncturing_vectors_have_eight_plus_n_ones() {
        for n in 1..=24 {
            assert_eq!(pi(n).iter().filter(|&&b| b).count(), 8 + n);
        }
        let p16: String = pi(16).iter().map(|&b| if b { '1' } else { '0' }).collect();
        assert_eq!(p16, "11101110111011101110111011101110");
        let p5: String = pi(5).iter().map(|&b| if b { '1' } else { '0' }).collect();
        assert_eq!(p5, "11001100110010001100100011001000");
    }

    #[test]
    fn viterbi_recovers_punctured_fic_block_with_errors() {
        let p = Puncturing {
            runs: vec![(21, 16), (3, 15)],
        };
        assert_eq!(p.coded_bits(), 2304);
        let info: Vec<u8> = (0..768u32).map(|i| ((i * 7 + i / 3) % 2) as u8).collect();
        let mut rx = punct(&encode(&info), &p);
        for i in (0..rx.len()).step_by(37) {
            rx[i] = -rx[i];
        }
        assert_eq!(viterbi(&p.depuncture(&rx)), info);
    }

    #[test]
    fn prbs_starts_with_known_sequence() {
        let mut bits = vec![0u8; 16];
        descramble(&mut bits);
        assert_eq!(pack(&bits), vec![0x07, 0xbe]);
    }

    #[test]
    fn crc_roundtrip() {
        let mut d = b"123456789".to_vec();
        assert_eq!(crc16(&d), 0x29b1);
        let c = crc16(&d) ^ 0xffff;
        d.extend_from_slice(&c.to_be_bytes());
        assert!(crc16_ok(&d));
    }
}
