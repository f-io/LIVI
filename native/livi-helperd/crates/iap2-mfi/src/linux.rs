use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::thread::sleep;
use std::time::Instant;

use crate::*;

/// Linux i2c ioctl — bind this fd to a target slave address (u16).
const I2C_SLAVE: libc::c_ulong = 0x0703;

const GPIO_CHIP: &str = "/dev/gpiochip0";
const GPIO_V2_LINE_FLAG_OUTPUT: u64 = 1 << 3;
// _IOWR(0xB4, nr, size)
const GPIO_V2_GET_LINE_IOCTL: u32 = (3 << 30) | ((size_of::<LineRequest>() as u32) << 16) | (0xB4 << 8) | 0x07;
const GPIO_V2_LINE_SET_VALUES_IOCTL: u32 = (3 << 30) | ((size_of::<LineValues>() as u32) << 16) | (0xB4 << 8) | 0x0F;

// The chip only leaves a wedged state on a real power cycle
const POWER_OFF_SETTLE: Duration = Duration::from_millis(50);
const POWER_ON_SETTLE: Duration = Duration::from_millis(100);

/// struct gpio_v2_line_request, linux/gpio.h
#[repr(C)]
struct LineRequest {
    offsets: [u32; 64],
    consumer: [u8; 32],
    flags: u64,
    num_attrs: u32,
    config_padding: [u32; 5],
    attrs: [[u64; 3]; 10],
    num_lines: u32,
    event_buffer_size: u32,
    padding: [u32; 5],
    fd: i32,
}

/// struct gpio_v2_line_values
#[repr(C)]
struct LineValues {
    bits: u64,
    mask: u64,
}

const _: () = assert!(size_of::<LineRequest>() == 592 && size_of::<LineValues>() == 16);

/// The chip's supply, held for as long as the line's fd is open.
struct PowerLine(fs::File);

impl PowerLine {
    fn on(gpio: u32) -> Result<Self, MfiError> {
        let chip = fs::OpenOptions::new()
            .read(true).write(true)
            .open(GPIO_CHIP)
            .map_err(|e| MfiError::Io(format!("open {GPIO_CHIP}: {e}")))?;
        let mut req: LineRequest = unsafe { std::mem::zeroed() };
        req.offsets[0] = gpio;
        req.num_lines = 1;
        req.flags = GPIO_V2_LINE_FLAG_OUTPUT;
        req.consumer[..8].copy_from_slice(b"livi-mfi");
        if unsafe { libc::ioctl(chip.as_raw_fd(), GPIO_V2_GET_LINE_IOCTL as _, &mut req) } < 0 {
            return Err(MfiError::Io(format!("claim gpio {gpio}: {}", std::io::Error::last_os_error())));
        }
        let line = Self(unsafe { <fs::File as std::os::fd::FromRawFd>::from_raw_fd(req.fd) });
        sleep(POWER_OFF_SETTLE);
        line.set(true).map_err(|e| MfiError::Io(format!("power gpio {gpio}: {e}")))?;
        sleep(POWER_ON_SETTLE);
        Ok(line)
    }

    fn set(&self, on: bool) -> std::io::Result<()> {
        let mut values = LineValues { bits: u64::from(on), mask: 1 };
        if unsafe { libc::ioctl(self.0.as_raw_fd(), GPIO_V2_LINE_SET_VALUES_IOCTL as _, &mut values) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for PowerLine {
    fn drop(&mut self) {
        let _ = self.set(false);
    }
}

pub struct I2cCoprocessor {
    file: fs::File,
    addr: u16,
    protocol_major: Option<u8>,
    power: Option<(u32, PowerLine)>,
}

impl I2cCoprocessor {
    /// Open `/dev/i2c-<bus>` and bind to the first responding MFi address
    /// (0x10 or 0x11 per `DEV_ADDR_CANDIDATES`). A negative `power_gpio` is a chip on the
    /// board's supply rail.
    pub fn open(bus: u32, power_gpio: i32) -> Result<Self, MfiError> {
        let power = match u32::try_from(power_gpio) {
            Ok(gpio) => Some((gpio, PowerLine::on(gpio)?)),
            Err(_) => None,
        };
        let bus_path = format!("/dev/i2c-{bus}");
        let addr = Self::probe(&bus_path)?;
        let file = fs::OpenOptions::new()
            .read(true).write(true)
            .open(&bus_path)
            .map_err(|e| MfiError::Io(format!("open {bus_path}: {e}")))?;
        set_slave(&file, addr)?;
        let mut chip = Self { file, addr, protocol_major: None, power };
        chip.protocol_major = chip.read_reg(REG_PROTOCOL_MAJOR, 1).ok().map(|v| v[0]);
        Ok(chip)
    }

    pub fn address(&self) -> u16 { self.addr }

    pub fn power_gpio(&self) -> Option<u32> { self.power.as_ref().map(|(gpio, _)| *gpio) }

    pub fn device_version(&mut self) -> Result<u8, MfiError> {
        Ok(self.read_reg(REG_DEVICE_VERSION, 1)?[0])
    }

    fn probe(bus_path: &str) -> Result<u16, MfiError> {
        let deadline = Instant::now() + PROBE_TIMEOUT;
        while Instant::now() < deadline {
            for cand in DEV_ADDR_CANDIDATES {
                let Ok(mut f) = fs::OpenOptions::new().read(true).write(true).open(bus_path) else {
                    continue;
                };
                if set_slave(&f, cand).is_err() { continue; }
                if f.write_all(&[REG_DEVICE_VERSION]).is_err() { continue; }
                let mut buf = [0u8; 1];
                if f.read_exact(&mut buf).is_ok() {
                    return Ok(cand);
                }
            }
            sleep(BUSY_RETRY);
        }
        Err(MfiError::NoChip { probed: DEV_ADDR_CANDIDATES.to_vec() })
    }

    fn retry(&mut self, what: &str, mut op: impl FnMut(&mut fs::File) -> bool) -> Result<(), MfiError> {
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            if op(&mut self.file) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(MfiError::Timeout(what.to_string()));
            }
            sleep(BUSY_RETRY);
        }
    }

    fn read_reg(&mut self, reg: u8, n: usize) -> Result<Vec<u8>, MfiError> {
        self.retry(&format!("register select 0x{reg:02X}"), |f| f.write_all(&[reg]).is_ok())?;
        let mut buf = vec![0u8; n];
        self.retry(&format!("read at 0x{reg:02X}"), |f| f.read_exact(&mut buf).is_ok())?;
        Ok(buf)
    }

    fn write_reg(&mut self, reg: u8, data: &[u8]) -> Result<(), MfiError> {
        let mut frame = Vec::with_capacity(data.len() + 1);
        frame.push(reg);
        frame.extend_from_slice(data);
        self.retry(&format!("write at 0x{reg:02X}"), |f| f.write_all(&frame).is_ok())
    }

    fn read_len(&mut self, reg: u8) -> Result<usize, MfiError> {
        let v = self.read_reg(reg, 2)?;
        Ok(u16::from_be_bytes([v[0], v[1]]) as usize)
    }
}

fn set_slave(file: &fs::File, addr: u16) -> Result<(), MfiError> {
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), I2C_SLAVE as _, addr as libc::c_ulong) };
    if rc < 0 {
        Err(MfiError::Io(format!(
            "I2C_SLAVE 0x{addr:02X}: {}",
            std::io::Error::last_os_error()
        )))
    } else {
        Ok(())
    }
}

impl AuthCoprocessor for I2cCoprocessor {
    fn protocol_major(&mut self) -> Result<u8, MfiError> {
        match self.protocol_major {
            Some(v) => Ok(v),
            None => {
                let v = self.read_reg(REG_PROTOCOL_MAJOR, 1)?[0];
                self.protocol_major = Some(v);
                Ok(v)
            }
        }
    }

    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        let size = self.read_len(REG_CERT_LENGTH)?;
        self.read_reg(REG_CERT_DATA, size)
    }

    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        let n = challenge.len();
        if !(CHALLENGE_MIN..=CHALLENGE_MAX).contains(&n) {
            return Err(MfiError::ChallengeSize(n));
        }
        self.write_reg(REG_CHALLENGE_LENGTH, &(n as u16).to_be_bytes())?;
        self.write_reg(REG_CHALLENGE_DATA, challenge)?;
        self.write_reg(REG_AUTH_CONTROL_STATUS, &[AUTH_START])?;

        sleep(Duration::from_millis(10));
        let deadline = Instant::now() + AUTH_TIMEOUT;
        loop {
            if let Ok(status) = self.read_reg(REG_AUTH_CONTROL_STATUS, 1)
                && status[0] == AUTH_DONE
            {
                break;
            }
            if Instant::now() >= deadline {
                let error_code = self.read_reg(REG_ERROR_CODE, 1).ok().map(|v| v[0]);
                return Err(MfiError::AuthFailed { error_code });
            }
            sleep(AUTH_POLL);
        }

        let size = self.read_len(REG_SIGNATURE_LENGTH)?;
        self.read_reg(REG_SIGNATURE_DATA, size)
    }
}
