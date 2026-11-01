// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod r820t;
mod regs;

use std::fmt;
use std::time::Duration;

use nusb::transfer::{Buffer, Bulk, In, TransferError};
use nusb::{DeviceInfo, Endpoint, Interface, MaybeFuture};

use r820t::R820t;
use regs::*;

pub use r820t::gains;

const KNOWN_IDS: [(u16, u16); 2] = [(0x0bda, 0x2838), (0x0bda, 0x2832)];
const XTAL: u32 = 28_800_000;
const BULK_IN: u8 = 0x81;

#[derive(Debug)]
pub enum Error {
    Usb(nusb::Error),
    Transfer(TransferError),
    ShortRead,
    UnknownTuner,
    Tuner(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usb(e) => write!(f, "usb: {e}"),
            Self::Transfer(e) => write!(f, "transfer: {e}"),
            Self::ShortRead => f.write_str("short control read"),
            Self::UnknownTuner => f.write_str("tuner is not an R820T/R820T2"),
            Self::Tuner(e) => write!(f, "tuner: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<nusb::Error> for Error {
    fn from(e: nusb::Error) -> Self {
        Self::Usb(e)
    }
}

impl From<TransferError> for Error {
    fn from(e: TransferError) -> Self {
        Self::Transfer(e)
    }
}

pub fn devices() -> Result<Vec<DeviceInfo>, Error> {
    Ok(nusb::list_devices()
        .wait()?
        .filter(|d| KNOWN_IDS.contains(&(d.vendor_id(), d.product_id())))
        .collect())
}

pub struct RtlSdr {
    regs: Regs,
    iface: Interface,
    tuner: R820t,
    rate: u32,
    freq: u32,
    ppm: i32,
}

impl RtlSdr {
    pub fn open(info: &DeviceInfo) -> Result<Self, Error> {
        let dev = info.open().wait()?;
        let iface = dev.detach_and_claim_interface(0).wait()?;
        let regs = Regs::new(dev);
        let mut sdr = Self {
            regs,
            iface,
            tuner: R820t::new(XTAL),
            rate: 0,
            freq: 0,
            ppm: 0,
        };
        sdr.init_baseband()?;

        sdr.regs.i2c_repeater(true)?;
        let tuner = sdr.probe_tuner().and_then(|()| {
            sdr.regs.demod_write(1, 0xb1, 0x1a, 1)?;
            sdr.regs.demod_write(0, 0x08, 0x4d, 1)?;
            sdr.set_if_freq(sdr.tuner.if_freq)?;
            sdr.regs.demod_write(1, 0x15, 0x01, 1)?;
            sdr.tuner.init(&sdr.regs)
        });
        sdr.regs.i2c_repeater(false)?;
        tuner?;

        sdr.set_sample_rate(2_048_000)?;
        Ok(sdr)
    }

    fn probe_tuner(&self) -> Result<(), Error> {
        self.regs.i2c_write(r820t::I2C_ADDR, &[0x00])?;
        match self.regs.i2c_read(r820t::I2C_ADDR, 1)?.first() {
            Some(&r820t::CHIP_ID) => Ok(()),
            _ => Err(Error::UnknownTuner),
        }
    }

    fn init_baseband(&self) -> Result<(), Error> {
        let r = &self.regs;
        r.write_reg(BLOCK_USB, USB_SYSCTL, 0x09, 1)?;
        r.write_reg(BLOCK_USB, USB_EPA_MAXPKT, 0x0002, 2)?;
        r.write_reg(BLOCK_USB, USB_EPA_CTL, 0x1002, 2)?;
        r.write_reg(BLOCK_SYS, SYS_DEMOD_CTL_1, 0x22, 1)?;
        r.write_reg(BLOCK_SYS, SYS_DEMOD_CTL, 0xe8, 1)?;
        r.demod_write(1, 0x01, 0x14, 1)?;
        r.demod_write(1, 0x01, 0x10, 1)?;
        r.demod_write(1, 0x15, 0x00, 1)?;
        r.demod_write(1, 0x16, 0x0000, 2)?;
        for i in 0..6 {
            r.demod_write(1, 0x16 + i, 0x00, 1)?;
        }
        for (i, b) in fir_bytes().iter().enumerate() {
            r.demod_write(1, 0x1c + i as u16, *b as u16, 1)?;
        }
        r.demod_write(0, 0x19, 0x05, 1)?;
        r.demod_write(1, 0x93, 0xf0, 1)?;
        r.demod_write(1, 0x94, 0x0f, 1)?;
        r.demod_write(1, 0x11, 0x00, 1)?;
        r.demod_write(1, 0x04, 0x00, 1)?;
        r.demod_write(0, 0x61, 0x60, 1)?;
        r.demod_write(0, 0x06, 0x80, 1)?;
        r.demod_write(1, 0xb1, 0x1b, 1)?;
        r.demod_write(0, 0x0d, 0x83, 1)
    }

    fn xtal(&self) -> u32 {
        (XTAL as f64 * (1.0 + self.ppm as f64 / 1e6)) as u32
    }

    fn set_if_freq(&self, freq: u32) -> Result<(), Error> {
        let v = -((freq as i64 * (1 << 22)) / self.xtal() as i64);
        self.regs
            .demod_write(1, 0x19, ((v >> 16) & 0x3f) as u16, 1)?;
        self.regs
            .demod_write(1, 0x1a, ((v >> 8) & 0xff) as u16, 1)?;
        self.regs.demod_write(1, 0x1b, (v & 0xff) as u16, 1)
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Error> {
        let xtal = self.xtal() as u64;
        let ratio = ((xtal << 22) / rate as u64) as u32 & 0x0fff_fffc;
        let real_ratio = ratio | ((ratio & 0x0800_0000) << 1);
        self.rate = ((xtal << 22) / real_ratio as u64) as u32;

        self.regs.i2c_repeater(true)?;
        let bw = self.tuner.set_bandwidth(&self.regs, self.rate);
        self.regs.i2c_repeater(false)?;
        self.set_if_freq(bw?)?;
        if self.freq != 0 {
            self.set_center_freq(self.freq)?;
        }

        self.regs.demod_write(1, 0x9f, (ratio >> 16) as u16, 2)?;
        self.regs.demod_write(1, 0xa1, ratio as u16, 2)?;
        self.write_ppm()?;
        self.regs.demod_write(1, 0x01, 0x14, 1)?;
        self.regs.demod_write(1, 0x01, 0x10, 1)
    }

    pub fn set_center_freq(&mut self, freq: u32) -> Result<(), Error> {
        self.regs.i2c_repeater(true)?;
        let r = self.tuner.set_freq(&self.regs, freq);
        self.regs.i2c_repeater(false)?;
        r?;
        self.freq = freq;
        Ok(())
    }

    pub fn set_gain(&mut self, gain: i32) -> Result<(), Error> {
        self.regs.i2c_repeater(true)?;
        let r = self.tuner.set_gain(&self.regs, gain);
        self.regs.i2c_repeater(false)?;
        r
    }

    pub fn set_ppm(&mut self, ppm: i32) -> Result<(), Error> {
        self.ppm = ppm;
        self.tuner.xtal = self.xtal();
        self.set_sample_rate(self.rate)
    }

    fn write_ppm(&self) -> Result<(), Error> {
        let offs = (-(self.ppm as i64) * (1 << 24) / 1_000_000) as i16;
        self.regs.demod_write(1, 0x3f, (offs & 0xff) as u16, 1)?;
        self.regs
            .demod_write(1, 0x3e, ((offs >> 8) & 0x3f) as u16, 1)
    }

    pub fn stream(&self, buffers: usize, len: usize) -> Result<IqStream, Error> {
        self.regs.write_reg(BLOCK_USB, USB_EPA_CTL, 0x1002, 2)?;
        self.regs.write_reg(BLOCK_USB, USB_EPA_CTL, 0x0000, 2)?;
        let mut ep = self.iface.endpoint::<Bulk, In>(BULK_IN)?;
        for _ in 0..buffers {
            let buf = ep.allocate(len);
            ep.submit(buf);
        }
        Ok(IqStream { ep, held: None })
    }
}

impl Drop for RtlSdr {
    fn drop(&mut self) {
        let _ = self.regs.i2c_repeater(true);
        let _ = self.tuner.standby(&self.regs);
        let _ = self.regs.i2c_repeater(false);
        let _ = self.regs.write_reg(BLOCK_SYS, SYS_DEMOD_CTL, 0x20, 1);
    }
}

pub struct IqStream {
    ep: Endpoint<Bulk, In>,
    held: Option<Buffer>,
}

impl IqStream {
    pub fn next_block(&mut self, timeout: Duration) -> Result<&[u8], Error> {
        if let Some(mut buf) = self.held.take() {
            buf.clear();
            self.ep.submit(buf);
        }
        let done = self
            .ep
            .wait_next_complete(timeout)
            .ok_or(TransferError::Cancelled)?;
        let buf = done.into_result()?;
        Ok(&self.held.insert(buf)[..])
    }
}

impl Drop for IqStream {
    fn drop(&mut self) {
        self.ep.cancel_all();
        while self.ep.pending() > 0 {
            if self
                .ep
                .wait_next_complete(Duration::from_millis(500))
                .is_none()
            {
                break;
            }
        }
    }
}

const FIR: [i16; 16] = [
    -54, -36, -41, -40, -32, -14, 14, 53, 101, 156, 215, 273, 327, 372, 404, 421,
];

fn fir_bytes() -> [u8; 20] {
    let mut out = [0u8; 20];
    for (o, t) in out.iter_mut().zip(&FIR[..8]) {
        *o = *t as u8;
    }
    for (n, pair) in FIR[8..].chunks(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let o = 8 + n * 3;
        out[o] = (a >> 4) as u8;
        out[o + 1] = ((a << 4) | ((b >> 8) & 0x0f)) as u8;
        out[o + 2] = b as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fir_packs_twelve_bit_pairs() {
        let b = fir_bytes();
        assert_eq!(b[0], (-54i16) as u8);
        assert_eq!(&b[8..11], &[0x06, 0x50, 0x9c]);
        assert_eq!(&b[17..20], &[0x19, 0x41, 0xa5]);
    }
}
