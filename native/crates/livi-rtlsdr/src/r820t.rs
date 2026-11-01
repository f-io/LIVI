// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::thread::sleep;
use std::time::Duration;

use crate::Error;
use crate::regs::Regs;

pub const I2C_ADDR: u8 = 0x34;
pub const CHIP_ID: u8 = 0x69;

const FIRST_REG: usize = 0x05;
const INIT: [u8; 27] = [
    0x83, 0x32, 0x75, 0xc0, 0x40, 0xd6, 0x6c, 0xf5, 0x63, 0x75, 0x68, 0x6c, 0x83, 0x80, 0x00, 0x0f,
    0x00, 0xc0, 0x30, 0x48, 0xcc, 0x60, 0x00, 0x54, 0xae, 0x4a, 0xc0,
];
const MAX_WRITE: usize = 7;
const VCO_MIN_KHZ: u32 = 1_770_000;
const VCO_POWER_REF: u8 = 2;
const FILTER_CAL_LO_KHZ: u32 = 56_000;

const MUX: [(u32, u8, u8, u8); 21] = [
    (0, 0x08, 0x02, 0xdf),
    (50, 0x08, 0x02, 0xbe),
    (55, 0x08, 0x02, 0x8b),
    (60, 0x08, 0x02, 0x7b),
    (65, 0x08, 0x02, 0x69),
    (70, 0x08, 0x02, 0x58),
    (75, 0x00, 0x02, 0x44),
    (80, 0x00, 0x02, 0x44),
    (90, 0x00, 0x02, 0x34),
    (100, 0x00, 0x02, 0x34),
    (110, 0x00, 0x02, 0x24),
    (120, 0x00, 0x02, 0x24),
    (140, 0x00, 0x02, 0x14),
    (180, 0x00, 0x02, 0x13),
    (220, 0x00, 0x02, 0x13),
    (250, 0x00, 0x02, 0x11),
    (280, 0x00, 0x02, 0x00),
    (310, 0x00, 0x41, 0x00),
    (450, 0x00, 0x41, 0x00),
    (588, 0x00, 0x40, 0x00),
    (650, 0x00, 0x40, 0x00),
];

const LNA_STEPS: [i32; 16] = [0, 9, 13, 40, 38, 13, 31, 22, 26, 31, 26, 14, 19, 5, 35, 13];
const MIXER_STEPS: [i32; 16] = [0, 5, 10, 10, 19, 9, 10, 25, 17, 10, 8, 16, 13, 6, 3, -8];

const IF_LOW_PASS: [u32; 10] = [
    1_700_000, 1_600_000, 1_550_000, 1_450_000, 1_200_000, 900_000, 700_000, 550_000, 450_000,
    350_000,
];
const HIGH_PASS_1: u32 = 350_000;
const HIGH_PASS_2: u32 = 380_000;

pub struct R820t {
    shadow: [u8; 32],
    pub xtal: u32,
    pub if_freq: u32,
}

impl R820t {
    pub fn new(xtal: u32) -> Self {
        Self {
            shadow: [0; 32],
            xtal,
            if_freq: 3_570_000,
        }
    }

    fn write(&mut self, regs: &Regs, reg: usize, data: &[u8]) -> Result<(), Error> {
        for (n, chunk) in data.chunks(MAX_WRITE).enumerate() {
            let start = reg + n * MAX_WRITE;
            let mut msg = Vec::with_capacity(chunk.len() + 1);
            msg.push(start as u8);
            msg.extend_from_slice(chunk);
            regs.i2c_write(I2C_ADDR, &msg)?;
            self.shadow[start..start + chunk.len()].copy_from_slice(chunk);
        }
        Ok(())
    }

    fn set(&mut self, regs: &Regs, reg: usize, val: u8) -> Result<(), Error> {
        self.write(regs, reg, &[val])
    }

    fn set_mask(&mut self, regs: &Regs, reg: usize, val: u8, mask: u8) -> Result<(), Error> {
        let merged = (self.shadow[reg] & !mask) | (val & mask);
        self.write(regs, reg, &[merged])
    }

    fn read(&self, regs: &Regs, len: usize) -> Result<Vec<u8>, Error> {
        regs.i2c_write(I2C_ADDR, &[0x00])?;
        Ok(regs
            .i2c_read(I2C_ADDR, len)?
            .into_iter()
            .map(u8::reverse_bits)
            .collect())
    }

    pub fn init(&mut self, regs: &Regs) -> Result<(), Error> {
        self.write(regs, FIRST_REG, &INIT)?;
        self.set_standard(regs)?;
        self.select_system(regs)
    }

    fn set_standard(&mut self, regs: &Regs) -> Result<(), Error> {
        self.set_mask(regs, 0x0c, 0x00, 0x0f)?;
        self.set_mask(regs, 0x13, 49, 0x3f)?;
        self.set_mask(regs, 0x1d, 0x00, 0x38)?;
        sleep(Duration::from_millis(1));

        let hp_cor = 0x6b;
        let mut cal_code = 0;
        for _ in 0..2 {
            self.set_mask(regs, 0x0b, hp_cor, 0x60)?;
            self.set_mask(regs, 0x0f, 0x04, 0x04)?;
            self.set_mask(regs, 0x10, 0x00, 0x03)?;
            if !self.set_pll(regs, FILTER_CAL_LO_KHZ * 1000)? {
                return Err(Error::Tuner("filter calibration PLL did not lock"));
            }
            self.set_mask(regs, 0x0b, 0x10, 0x10)?;
            sleep(Duration::from_millis(1));
            self.set_mask(regs, 0x0b, 0x00, 0x10)?;
            self.set_mask(regs, 0x0f, 0x00, 0x04)?;
            cal_code = self.read(regs, 5)?[4] & 0x0f;
            if cal_code != 0 && cal_code != 0x0f {
                break;
            }
        }
        if cal_code == 0x0f {
            cal_code = 0;
        }

        self.set_mask(regs, 0x0a, 0x10 | cal_code, 0x1f)?;
        self.set_mask(regs, 0x0b, hp_cor, 0xef)?;
        self.set_mask(regs, 0x07, 0x00, 0x80)?;
        self.set_mask(regs, 0x06, 0x10, 0x30)?;
        self.set_mask(regs, 0x1e, 0x60, 0x60)?;
        self.set_mask(regs, 0x05, 0x01, 0x80)?;
        self.set_mask(regs, 0x1f, 0x00, 0x80)?;
        self.set_mask(regs, 0x0f, 0x00, 0x80)?;
        self.set_mask(regs, 0x19, 0x60, 0x60)
    }

    fn select_system(&mut self, regs: &Regs) -> Result<(), Error> {
        let mixer_top = 0x24;
        self.set_mask(regs, 0x1d, 0xe5, 0xc7)?;
        self.set_mask(regs, 0x1c, mixer_top, 0xf8)?;
        self.set(regs, 0x0d, 0x53)?;
        self.set(regs, 0x0e, 0x75)?;
        self.set_mask(regs, 0x05, 0x00, 0x60)?;
        self.set_mask(regs, 0x06, 0x00, 0x08)?;
        self.set_mask(regs, 0x11, 0x38, 0x38)?;
        self.set_mask(regs, 0x17, 0x30, 0x30)?;
        self.set_mask(regs, 0x0a, 0x40, 0x60)?;

        self.set_mask(regs, 0x1d, 0x00, 0x38)?;
        self.set_mask(regs, 0x1c, 0x00, 0x04)?;
        self.set_mask(regs, 0x06, 0x00, 0x40)?;
        self.set_mask(regs, 0x1a, 0x30, 0x30)?;
        sleep(Duration::from_millis(250));
        self.set_mask(regs, 0x1d, 0x18, 0x38)?;
        self.set_mask(regs, 0x1c, mixer_top, 0x04)?;
        self.set_mask(regs, 0x1e, 14, 0x1f)?;
        self.set_mask(regs, 0x1a, 0x20, 0x30)
    }

    fn set_pll(&mut self, regs: &Regs, freq: u32) -> Result<bool, Error> {
        let freq_khz = (freq + 500) / 1000;
        let ref_khz = (self.xtal + 500) / 1000;

        self.set_mask(regs, 0x10, 0x00, 0x10)?;
        self.set_mask(regs, 0x1a, 0x00, 0x0c)?;
        self.set_mask(regs, 0x12, 0x80, 0xe0)?;

        let mut mix_div: u32 = 2;
        let mut div_num: u8 = 0;
        while mix_div <= 64 {
            let vco = freq_khz * mix_div;
            if (VCO_MIN_KHZ..VCO_MIN_KHZ * 2).contains(&vco) {
                let mut div_buf = mix_div;
                while div_buf > 2 {
                    div_buf >>= 1;
                    div_num += 1;
                }
                break;
            }
            mix_div <<= 1;
        }

        let fine_tune = (self.read(regs, 5)?[4] & 0x30) >> 4;
        if fine_tune > VCO_POWER_REF {
            div_num = div_num.wrapping_sub(1);
        } else if fine_tune < VCO_POWER_REF {
            div_num += 1;
        }
        self.set_mask(regs, 0x10, div_num << 5, 0xe0)?;

        let vco_freq = freq as u64 * mix_div as u64;
        let two_ref = 2 * self.xtal as u64;
        let nint = (vco_freq / two_ref) as u32;
        let mut vco_fra = ((vco_freq - two_ref * nint as u64) / 1000) as u32;
        if nint > 128 / VCO_POWER_REF as u32 - 1 {
            return Err(Error::Tuner("no PLL setting for this frequency"));
        }
        let ni = (nint - 13) / 4;
        let si = nint - 4 * ni - 13;
        self.set(regs, 0x14, (ni + (si << 6)) as u8)?;
        self.set_mask(regs, 0x12, if vco_fra == 0 { 0x08 } else { 0x00 }, 0x08)?;

        let mut n_sdm: u32 = 2;
        let mut sdm: u32 = 0;
        while vco_fra > 1 {
            if vco_fra > 2 * ref_khz / n_sdm {
                sdm += 32768 / (n_sdm / 2);
                vco_fra -= 2 * ref_khz / n_sdm;
                if n_sdm >= 0x8000 {
                    break;
                }
            }
            n_sdm <<= 1;
        }
        self.set(regs, 0x16, (sdm >> 8) as u8)?;
        self.set(regs, 0x15, sdm as u8)?;

        let mut locked = false;
        for attempt in 0..2 {
            sleep(Duration::from_millis(10));
            locked = self.read(regs, 3)?[2] & 0x40 != 0;
            if locked {
                break;
            }
            if attempt == 0 {
                self.set_mask(regs, 0x12, 0x60, 0xe0)?;
            }
        }
        if locked {
            self.set_mask(regs, 0x1a, 0x08, 0x08)?;
        }
        Ok(locked)
    }

    pub fn set_freq(&mut self, regs: &Regs, freq: u32) -> Result<(), Error> {
        let lo = freq + self.if_freq;
        let mhz = lo / 1_000_000;
        let i = MUX.iter().rposition(|m| mhz >= m.0).unwrap_or(0);
        let (_, open_d, rf_mux, tf_band) = MUX[i];
        self.set_mask(regs, 0x17, open_d, 0x08)?;
        self.set_mask(regs, 0x1a, rf_mux, 0xc3)?;
        self.set(regs, 0x1b, tf_band)?;
        self.set_mask(regs, 0x10, 0x00, 0x0b)?;
        self.set_mask(regs, 0x08, 0x00, 0x3f)?;
        self.set_mask(regs, 0x09, 0x00, 0x3f)?;
        if !self.set_pll(regs, lo)? {
            return Err(Error::Tuner("PLL did not lock"));
        }
        Ok(())
    }

    pub fn set_bandwidth(&mut self, regs: &Regs, bw: u32) -> Result<u32, Error> {
        let (reg_0a, reg_0b) = if bw > 7_000_000 {
            self.if_freq = 4_570_000;
            (0x10, 0x0b)
        } else if bw > 6_000_000 {
            self.if_freq = 4_570_000;
            (0x10, 0x2a)
        } else if bw > IF_LOW_PASS[0] + HIGH_PASS_1 + HIGH_PASS_2 {
            self.if_freq = 3_570_000;
            (0x10, 0x6b)
        } else {
            let mut bw = bw;
            let mut reg_0b = 0x80u8;
            let mut if_freq = 2_300_000;
            let mut real_bw = 0;
            if bw > IF_LOW_PASS[0] + HIGH_PASS_1 {
                bw -= HIGH_PASS_2;
                if_freq += HIGH_PASS_2;
                real_bw += HIGH_PASS_2;
            } else {
                reg_0b |= 0x20;
            }
            if bw > IF_LOW_PASS[0] {
                bw -= HIGH_PASS_1;
                if_freq += HIGH_PASS_1;
                real_bw += HIGH_PASS_1;
            } else {
                reg_0b |= 0x40;
            }
            let i = IF_LOW_PASS
                .iter()
                .position(|&lp| bw > lp)
                .unwrap_or(IF_LOW_PASS.len());
            let i = i.saturating_sub(1);
            reg_0b |= 15 - i as u8;
            real_bw += IF_LOW_PASS[i];
            self.if_freq = if_freq - real_bw / 2;
            (0x00, reg_0b)
        };
        self.set_mask(regs, 0x0a, reg_0a, 0x10)?;
        self.set_mask(regs, 0x0b, reg_0b, 0xef)?;
        Ok(self.if_freq)
    }

    pub fn set_gain(&mut self, regs: &Regs, gain: i32) -> Result<(), Error> {
        self.set_mask(regs, 0x05, 0x10, 0x10)?;
        self.set_mask(regs, 0x07, 0x00, 0x10)?;
        self.set_mask(regs, 0x0c, 0x08, 0x9f)?;
        let (lna, mixer) = gain_steps(gain);
        self.set_mask(regs, 0x05, lna, 0x0f)?;
        self.set_mask(regs, 0x07, mixer, 0x0f)
    }

    pub fn standby(&mut self, regs: &Regs) -> Result<(), Error> {
        for (reg, val) in [
            (0x06, 0xb1),
            (0x05, 0xa0),
            (0x07, 0x3a),
            (0x08, 0x40),
            (0x09, 0xc0),
            (0x0a, 0x36),
            (0x0c, 0x35),
            (0x0f, 0x68),
            (0x11, 0x03),
            (0x17, 0xf4),
            (0x19, 0x0c),
        ] {
            self.set(regs, reg, val)?;
        }
        Ok(())
    }
}

fn gain_steps(gain: i32) -> (u8, u8) {
    let (mut lna, mut mixer, mut total) = (0, 0, 0);
    for _ in 0..15 {
        if total >= gain {
            break;
        }
        lna += 1;
        total += LNA_STEPS[lna];
        if total >= gain {
            break;
        }
        mixer += 1;
        total += MIXER_STEPS[mixer];
    }
    (lna as u8, mixer as u8)
}

pub fn gains() -> Vec<i32> {
    let mut out = vec![0];
    let mut total = 0;
    for i in 1..16 {
        total += LNA_STEPS[i];
        out.push(total);
        total += MIXER_STEPS[i];
        out.push(total);
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_steps_stop_at_first_reach() {
        assert_eq!(gain_steps(0), (0, 0));
        assert_eq!(gain_steps(9), (1, 0));
        assert_eq!(gain_steps(14), (1, 1));
        assert_eq!(gain_steps(496), (15, 14));
    }

    #[test]
    fn gain_table_covers_full_range() {
        let g = gains();
        assert_eq!(g.first(), Some(&0));
        assert_eq!(g.last(), Some(&496));
        assert!(g.contains(&197));
    }
}
