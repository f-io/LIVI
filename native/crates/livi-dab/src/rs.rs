// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const PRIM: u16 = 0x11d;
const PARITY: usize = 10;
pub const N: usize = 120;
pub const K: usize = 110;

pub struct Gf {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Default for Gf {
    fn default() -> Self {
        Self::new()
    }
}

impl Gf {
    pub fn new() -> Self {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u16 = 1;
        for (i, e) in exp.iter_mut().take(255).enumerate() {
            *e = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= PRIM;
            }
        }
        exp.copy_within(0..257, 255);
        Self { exp, log }
    }

    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize]
    }

    fn div(&self, a: u8, b: u8) -> u8 {
        if a == 0 {
            return 0;
        }
        self.exp[(self.log[a as usize] as usize + 255 - self.log[b as usize] as usize) % 255]
    }

    fn pow(&self, e: usize) -> u8 {
        self.exp[e % 255]
    }

    fn eval(&self, p: &[u8], x: u8) -> u8 {
        p.iter().rev().fold(0, |acc, &c| self.mul(acc, x) ^ c)
    }

    pub fn correct(&self, cw: &mut [u8]) -> Option<usize> {
        let n = cw.len();
        let mut synd = [0u8; PARITY];
        let mut clean = true;
        for (j, s) in synd.iter_mut().enumerate() {
            let x = self.pow(j);
            *s = cw.iter().fold(0, |acc, &c| self.mul(acc, x) ^ c);
            clean &= *s == 0;
        }
        if clean {
            return Some(0);
        }

        let mut lambda = vec![1u8];
        let mut prev = vec![1u8];
        let mut l = 0;
        let mut m = 1;
        let mut b = 1u8;
        for r in 0..PARITY {
            let mut d = synd[r];
            for i in 1..=l.min(lambda.len() - 1) {
                d ^= self.mul(lambda[i], synd[r - i]);
            }
            if d == 0 {
                m += 1;
                continue;
            }
            let coef = self.div(d, b);
            let mut next = lambda.clone();
            if next.len() < prev.len() + m {
                next.resize(prev.len() + m, 0);
            }
            for (i, &p) in prev.iter().enumerate() {
                next[i + m] ^= self.mul(coef, p);
            }
            if 2 * l <= r {
                l = r + 1 - l;
                prev = lambda;
                b = d;
                m = 1;
            } else {
                m += 1;
            }
            lambda = next;
        }
        while lambda.last() == Some(&0) {
            lambda.pop();
        }
        if lambda.len() - 1 != l || l > PARITY / 2 {
            return None;
        }

        let mut positions = Vec::new();
        for deg in 0..n {
            let inv = self.pow(255 - deg % 255);
            if self.eval(&lambda, inv) == 0 {
                positions.push(deg);
            }
        }
        if positions.len() != l {
            return None;
        }

        let mut omega = vec![0u8; PARITY];
        for (i, o) in omega.iter_mut().enumerate() {
            for j in 0..=i.min(lambda.len() - 1) {
                *o ^= self.mul(lambda[j], synd[i - j]);
            }
        }
        let deriv: Vec<u8> = lambda
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, &c)| if i % 2 == 1 { c } else { 0 })
            .collect();
        for &deg in &positions {
            let x = self.pow(deg);
            let inv = self.pow(255 - deg % 255);
            let denom = self.eval(&deriv, inv);
            if denom == 0 {
                return None;
            }
            let mag = self.mul(x, self.div(self.eval(&omega, inv), denom));
            cw[n - 1 - deg] ^= mag;
        }
        Some(l)
    }

    #[cfg(test)]
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let mut g = vec![1u8];
        for i in 0..PARITY {
            let root = self.pow(i);
            let mut next = vec![0u8; g.len() + 1];
            for (j, &c) in g.iter().enumerate() {
                next[j] ^= self.mul(c, root);
                next[j + 1] ^= c;
            }
            g = next;
        }
        let mut rem = vec![0u8; PARITY];
        for &d in data {
            let f = d ^ rem[0];
            rem.remove(0);
            rem.push(0);
            for (k, r) in rem.iter_mut().enumerate() {
                *r ^= self.mul(f, g[PARITY - 1 - k]);
            }
        }
        let mut out = data.to_vec();
        out.extend_from_slice(&rem);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrects_up_to_five_errors() {
        let gf = Gf::new();
        let data: Vec<u8> = (0..K as u32).map(|i| (i * 31 + 7) as u8).collect();
        let cw = gf.encode(&data);
        assert_eq!(cw.len(), N);
        let mut clean = cw.clone();
        assert_eq!(gf.correct(&mut clean), Some(0));

        let mut bad = cw.clone();
        for (i, p) in [0usize, 17, 55, 109, 119].iter().enumerate() {
            bad[*p] ^= 0x5a + i as u8;
        }
        assert_eq!(gf.correct(&mut bad), Some(5));
        assert_eq!(bad, cw);
    }

    #[test]
    fn gives_up_on_six_errors() {
        let gf = Gf::new();
        let data = vec![0x42u8; K];
        let mut bad = gf.encode(&data);
        for p in [1usize, 2, 3, 4, 5, 6] {
            bad[p] ^= 0xff;
        }
        let fixed = gf.correct(&mut bad);
        assert!(fixed.is_none() || bad != gf.encode(&data));
    }
}
