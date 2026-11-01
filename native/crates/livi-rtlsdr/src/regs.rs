// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use nusb::{Device, MaybeFuture};

use crate::Error;

const TIMEOUT: Duration = Duration::from_millis(300);

pub const BLOCK_USB: u16 = 1;
pub const BLOCK_SYS: u16 = 2;
const BLOCK_I2C: u16 = 6;

pub const USB_SYSCTL: u16 = 0x2000;
pub const USB_EPA_CTL: u16 = 0x2148;
pub const USB_EPA_MAXPKT: u16 = 0x2158;
pub const SYS_DEMOD_CTL: u16 = 0x3000;
pub const SYS_DEMOD_CTL_1: u16 = 0x300b;

pub struct Regs {
    dev: Device,
}

impl Regs {
    pub fn new(dev: Device) -> Self {
        Self { dev }
    }

    fn read(&self, value: u16, index: u16, len: usize) -> Result<Vec<u8>, Error> {
        let data = self
            .dev
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value,
                    index,
                    length: len as u16,
                },
                TIMEOUT,
            )
            .wait()?;
        if data.len() < len {
            return Err(Error::ShortRead);
        }
        Ok(data)
    }

    fn write(&self, value: u16, index: u16, data: &[u8]) -> Result<(), Error> {
        self.dev
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value,
                    index,
                    data,
                },
                TIMEOUT,
            )
            .wait()?;
        Ok(())
    }

    pub fn write_reg(&self, block: u16, addr: u16, val: u16, len: usize) -> Result<(), Error> {
        let bytes = val.to_be_bytes();
        self.write(addr, (block << 8) | 0x10, &bytes[2 - len..])
    }

    pub fn demod_write(&self, page: u8, addr: u16, val: u16, len: usize) -> Result<(), Error> {
        let bytes = val.to_be_bytes();
        self.write((addr << 8) | 0x20, 0x10 | page as u16, &bytes[2 - len..])?;
        self.demod_read(0x0a, 0x01)?;
        Ok(())
    }

    pub fn demod_read(&self, page: u8, addr: u16) -> Result<u8, Error> {
        Ok(self.read((addr << 8) | 0x20, page as u16, 1)?[0])
    }

    pub fn i2c_write(&self, i2c_addr: u8, data: &[u8]) -> Result<(), Error> {
        self.write(i2c_addr as u16, (BLOCK_I2C << 8) | 0x10, data)
    }

    pub fn i2c_read(&self, i2c_addr: u8, len: usize) -> Result<Vec<u8>, Error> {
        self.read(i2c_addr as u16, BLOCK_I2C << 8, len)
    }

    pub fn i2c_repeater(&self, on: bool) -> Result<(), Error> {
        self.demod_write(1, 0x01, if on { 0x18 } else { 0x10 }, 1)
    }
}
