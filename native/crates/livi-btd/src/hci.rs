// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    const BTPROTO_HCI: libc::c_int = 1;
    const HCIDEVUP: libc::c_ulong = 0x4004_48c9;
    const HCIDEVDOWN: libc::c_ulong = 0x4004_48ca;
    const HCIGETDEVINFO: libc::c_ulong = 0x8004_48d3;
    const HCI_UP: u32 = 1 << 0;

    #[repr(C)]
    struct HciDevInfo {
        dev_id: u16,
        name: [u8; 8],
        bdaddr: [u8; 6],
        flags: u32,
        dev_type: u8,
        _pad: [u8; 7],
        features: [u8; 8],
        _rest: [u8; 128],
    }

    fn control_socket() -> io::Result<OwnedFd> {
        let raw = unsafe {
            libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                BTPROTO_HCI,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }

    fn dev_ioctl(request: libc::c_ulong, dev: u16, tolerated: Option<i32>) -> io::Result<()> {
        let sock = control_socket()?;
        let r = unsafe { libc::ioctl(sock.as_raw_fd(), request as _, dev as libc::c_int) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != tolerated {
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn up(dev: u16) -> io::Result<()> {
        dev_ioctl(HCIDEVUP, dev, Some(libc::EALREADY))
    }

    pub fn down(dev: u16) -> io::Result<()> {
        dev_ioctl(HCIDEVDOWN, dev, Some(libc::EALREADY))
    }

    fn dev_info(dev: u16) -> io::Result<HciDevInfo> {
        let sock = control_socket()?;
        let mut info: HciDevInfo = unsafe { std::mem::zeroed() };
        info.dev_id = dev;
        let r = unsafe { libc::ioctl(sock.as_raw_fd(), HCIGETDEVINFO as _, &raw mut info) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    pub fn is_up(dev: u16) -> bool {
        dev_info(dev).is_ok_and(|info| (info.flags & HCI_UP) != 0)
    }

    pub fn info(dev: u16) -> io::Result<super::DevInfo> {
        dev_info(dev).map(|info| super::DevInfo {
            bdaddr: info.bdaddr,
            dev_type: info.dev_type,
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::io;

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "hci ioctls are linux only")
    }

    pub fn up(_dev: u16) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn down(_dev: u16) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn is_up(_dev: u16) -> bool {
        false
    }

    pub fn info(_dev: u16) -> io::Result<super::DevInfo> {
        Err(unsupported())
    }
}

pub struct DevInfo {
    pub bdaddr: [u8; 6],
    pub dev_type: u8,
}

pub use imp::{down, info, is_up, up};
