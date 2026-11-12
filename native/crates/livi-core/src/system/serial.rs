// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use tokio::sync::mpsc;

const WAKE_MS: i32 = 250;

pub type Chunk = Result<Vec<u8>, String>;

pub struct Port {
    file: File,
    pub chunks: mpsc::UnboundedReceiver<Chunk>,
}

impl Port {
    pub fn open(device: &str, baud_rate: u32) -> Result<Self, String> {
        if !Path::new(device).exists() {
            return Err(format!("{device} not found"));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(device)
            .map_err(|e| format!("open failed: {e}"))?;
        raw_mode(&file, baud_rate).map_err(|e| format!("serial setup failed: {e}"))?;
        let reader = file.try_clone().map_err(|e| format!("open failed: {e}"))?;
        let (tx, chunks) = mpsc::unbounded_channel();
        let device = device.to_string();
        std::thread::spawn(move || read_chunks(reader, &device, &tx));
        Ok(Self { file, chunks })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }

    #[cfg(test)]
    pub fn fake(chunks: mpsc::UnboundedReceiver<Chunk>) -> Self {
        Self {
            file: OpenOptions::new().write(true).open("/dev/null").unwrap(),
            chunks,
        }
    }
}

fn speed(baud_rate: u32) -> Option<libc::speed_t> {
    Some(match baud_rate {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        #[cfg(target_os = "linux")]
        460800 => libc::B460800,
        #[cfg(target_os = "linux")]
        921600 => libc::B921600,
        _ => return None,
    })
}

fn raw_mode(file: &File, baud_rate: u32) -> io::Result<()> {
    let speed = speed(baud_rate).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{baud_rate} baud is not supported"),
        )
    })?;
    let fd = file.as_raw_fd();
    // SAFETY: an all-zero termios is valid, tcgetattr fills it.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: fd is open for the call and t is a valid termios.
    unsafe {
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(io::Error::last_os_error());
        }
        libc::cfmakeraw(&mut t);
        t.c_cflag &= !libc::CRTSCTS;
        if libc::cfsetspeed(&mut t, speed) != 0 || libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn read_chunks(mut reader: File, device: &str, tx: &mpsc::UnboundedSender<Chunk>) {
    let mut buf = [0u8; 4096];
    let mut poll = libc::pollfd {
        fd: reader.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    while !tx.is_closed() {
        // SAFETY: poll gets one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, WAKE_MS) };
        if ready == 0 {
            continue;
        }
        let read = if ready < 0 {
            Err(io::Error::last_os_error())
        } else {
            reader.read(&mut buf)
        };
        let chunk = match read {
            Ok(0) => Err(format!("{device} closed")),
            Ok(n) => Ok(buf[..n].to_vec()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => Err(format!("read failed: {e}")),
        };
        let stop = chunk.is_err();
        if tx.send(chunk).is_err() || stop {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_device_and_a_plain_file_are_refused() {
        assert_eq!(
            Port::open("/nonexistent/gnss0", 38400).err().as_deref(),
            Some("/nonexistent/gnss0 not found")
        );
        let err = Port::open("/dev/null", 38400).err().unwrap();
        assert!(err.starts_with("serial setup failed:"), "{err}");
    }

    #[test]
    fn the_rates_the_settings_offer_are_known() {
        for rate in [4800, 9600, 19200, 38400, 57600, 115200] {
            assert!(speed(rate).is_some(), "{rate}");
        }
        assert_eq!(speed(12345), None);
    }
}
