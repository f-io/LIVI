//! `hciconfig hciN up|down` without hciconfig: the same ioctls on a raw HCI socket. The
//! V821B rootfs ships no bluez, and the CPC200 no longer needs its copy either.

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const AF_BLUETOOTH: libc::c_int = 31;
    const BTPROTO_HCI: libc::c_int = 1;
    // <net/bluetooth/hci.h>: _IOW('H', 201, int), _IOW('H', 202, int), _IOR('H', 211, int)
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
                AF_BLUETOOTH,
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

    pub fn is_up(dev: u16) -> bool {
        let Ok(sock) = control_socket() else {
            return false;
        };
        let mut info: HciDevInfo = unsafe { std::mem::zeroed() };
        info.dev_id = dev;
        let r = unsafe {
            libc::ioctl(sock.as_raw_fd(), HCIGETDEVINFO as _, &raw mut info)
        };
        r >= 0 && (info.flags & HCI_UP) != 0
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
}

pub use imp::{down, is_up, up};
