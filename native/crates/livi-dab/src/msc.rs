// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::fec::{Puncturing, descramble, pack, viterbi};
use crate::fic::{Protection, Subchannel};

const CU_BITS: usize = 64;
const DEPTH: usize = 16;
const DELAY: [usize; DEPTH] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];

pub fn eep_puncturing(sub: &Subchannel) -> Option<Puncturing> {
    let Protection::Eep { option, level } = sub.protection else {
        return None;
    };
    let size = sub.size as usize;
    let runs = match (option, level) {
        (0, level) => {
            let n = size / [12, 8, 6, 4][level as usize];
            if n == 0 {
                return None;
            }
            match level {
                0 => vec![(6 * n - 3, 24), (3, 23)],
                1 if n == 1 => vec![(5, 13), (1, 12)],
                1 => vec![(2 * n - 3, 14), (4 * n + 3, 13)],
                2 => vec![(6 * n - 3, 8), (3, 7)],
                _ if n == 1 => return None,
                _ => vec![(4 * n - 3, 3), (2 * n + 3, 2)],
            }
        }
        (1, level) => {
            let n = size / [27, 21, 18, 15][level as usize];
            if n == 0 {
                return None;
            }
            let pi = [10, 6, 4, 2][level as usize];
            vec![(24 * n - 3, pi), (3, pi - 1)]
        }
        _ => return None,
    };
    Some(Puncturing { runs })
}

pub struct SubchannelDecoder {
    sub: Subchannel,
    puncturing: Puncturing,
    ring: Vec<Vec<i8>>,
    index: usize,
    filled: usize,
}

impl SubchannelDecoder {
    pub fn new(sub: Subchannel) -> Option<Self> {
        let puncturing = eep_puncturing(&sub)?;
        let bits = sub.size as usize * CU_BITS;
        if puncturing.coded_bits() != bits {
            return None;
        }
        Some(Self {
            sub,
            puncturing,
            ring: vec![vec![0; bits]; DEPTH],
            index: 0,
            filled: 0,
        })
    }

    pub fn subchannel(&self) -> &Subchannel {
        &self.sub
    }

    pub fn process_cif(&mut self, cif: &[i8]) -> Option<Vec<u8>> {
        let start = self.sub.start as usize * CU_BITS;
        let input = cif.get(start..start + self.sub.size as usize * CU_BITS)?;
        let mut out = vec![0i8; input.len()];
        for (i, (o, &x)) in out.iter_mut().zip(input).enumerate() {
            *o = self.ring[(self.index + DELAY[i % DEPTH]) % DEPTH][i];
            self.ring[self.index][i] = x;
        }
        self.index = (self.index + 1) % DEPTH;
        if self.filled < DEPTH {
            self.filled += 1;
            return None;
        }
        let mut bits = viterbi(&self.puncturing.depuncture(&out));
        descramble(&mut bits);
        Some(pack(&bits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(size: u16, option: u8, level: u8) -> Subchannel {
        Subchannel {
            id: 1,
            start: 0,
            size,
            protection: Protection::Eep { option, level },
        }
    }

    #[test]
    fn every_eep_profile_fills_its_sub_channel() {
        for (option, level, unit) in [
            (0, 0, 12),
            (0, 1, 8),
            (0, 2, 6),
            (0, 3, 4),
            (1, 0, 27),
            (1, 1, 21),
            (1, 2, 18),
            (1, 3, 15),
        ] {
            for n in 1..=12u16 {
                let s = sub(unit * n, option, level);
                let Some(p) = eep_puncturing(&s) else {
                    continue;
                };
                assert_eq!(
                    p.coded_bits(),
                    s.size as usize * CU_BITS,
                    "{option} {level} {n}"
                );
                let rate = if option == 0 { 8 } else { 32 };
                assert_eq!(
                    p.info_bits(),
                    rate * n as usize * 24,
                    "{option} {level} {n}"
                );
            }
        }
    }
}
