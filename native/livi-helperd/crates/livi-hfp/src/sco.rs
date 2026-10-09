//! The phone's call audio, CVSD = raw PCM s16le 8 kHz.

use std::os::fd::{AsRawFd, OwnedFd};

/// The largest packet a controller hands over, its length field is one byte.
pub const MAX_PACKET: usize = 255;

/// A call's audio, a packet per read and per send. The Linux kernel's SCO socket, or the datagram
/// socket a host of our own hands out.
pub struct Link {
    pub fd: OwnedFd,
    pub mtu: usize,
    /// Reversed, the way the kernel stores an address.
    pub peer: [u8; 6],
}

impl Link {
    /// One whole packet, `packet` should hold MAX_PACKET. A controller may send more than its
    /// MTU, and the socket drops whatever does not fit.
    pub fn read(&self, packet: &mut [u8]) -> std::io::Result<usize> {
        let n =
            unsafe { libc::read(self.fd.as_raw_fd(), packet.as_mut_ptr().cast(), packet.len()) };
        usize::try_from(n).map_err(|_| std::io::Error::last_os_error())
    }

    /// In pieces of the MTU, the kernel refuses larger ones. A controller that takes no more audio
    /// must not hold up the caller's side, what it refuses is dropped.
    pub fn send(&self, pcm: &[u8]) -> usize {
        pcm.chunks(self.mtu)
            .map(|piece| {
                let sent = unsafe {
                    libc::send(
                        self.fd.as_raw_fd(),
                        piece.as_ptr().cast(),
                        piece.len(),
                        libc::MSG_DONTWAIT,
                    )
                };
                usize::try_from(sent).unwrap_or(0)
            })
            .sum()
    }
}

#[cfg(target_os = "linux")]
pub use kernel::{accept, listen};

#[cfg(target_os = "linux")]
mod kernel {
    use super::Link;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const BTPROTO_SCO: libc::c_int = 2;
    const SOL_SCO: libc::c_int = 17;
    const SCO_OPTIONS: libc::c_int = 1;
    const SOL_BLUETOOTH: libc::c_int = 274;
    const BT_DEFER_SETUP: libc::c_int = 7;
    const BT_VOICE: libc::c_int = 11;
    /// CVSD on the air, 16-bit signed linear PCM to and from the host.
    const VOICE_CVSD_16BIT: u16 = 0x0060;

    fn set<T>(
        fd: &OwnedFd,
        level: libc::c_int,
        name: libc::c_int,
        value: &T,
    ) -> std::io::Result<()> {
        let rc = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                level,
                name,
                std::ptr::from_ref(value).cast(),
                std::mem::size_of::<T>() as libc::socklen_t,
            )
        };
        if rc == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }

    #[repr(C)]
    struct SockaddrSco {
        sco_family: libc::sa_family_t,
        sco_bdaddr: [u8; 6],
    }

    #[repr(C)]
    struct ScoOptions {
        mtu: u16,
    }

    pub fn listen() -> std::io::Result<OwnedFd> {
        let raw = unsafe { libc::socket(libc::AF_BLUETOOTH, libc::SOCK_SEQPACKET, BTPROTO_SCO) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let addr =
            SockaddrSco { sco_family: libc::AF_BLUETOOTH as libc::sa_family_t, sco_bdaddr: [0; 6] };
        let rc = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                std::ptr::addr_of!(addr).cast(),
                std::mem::size_of::<SockaddrSco>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Deferred, so a call is accepted with the socket's voice setting. Without it the kernel
        // takes the controller's own, which on the AIC8800 is no 16-bit PCM.
        set(&fd, SOL_BLUETOOTH, BT_DEFER_SETUP, &1u32)?;
        if unsafe { libc::listen(fd.as_raw_fd(), 1) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(fd)
    }

    pub fn accept(listen: &OwnedFd) -> std::io::Result<Link> {
        let mut addr = SockaddrSco { sco_family: 0, sco_bdaddr: [0; 6] };
        let mut size = std::mem::size_of::<SockaddrSco>() as libc::socklen_t;
        let raw = unsafe {
            libc::accept(listen.as_raw_fd(), std::ptr::addr_of_mut!(addr).cast(), &raw mut size)
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        set(&fd, SOL_BLUETOOTH, BT_VOICE, &VOICE_CVSD_16BIT)?;
        // The first read accepts the deferred call and returns nothing.
        let mut first = [0u8; 1];
        if unsafe { libc::read(fd.as_raw_fd(), first.as_mut_ptr().cast(), 1) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut opts = ScoOptions { mtu: 48 };
        let mut len = std::mem::size_of::<ScoOptions>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                SOL_SCO,
                SCO_OPTIONS,
                std::ptr::addr_of_mut!(opts).cast(),
                &raw mut len,
            );
        }
        Ok(Link { fd, mtu: usize::from(opts.mtu.max(24)), peer: addr.sco_bdaddr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixDatagram;

    fn pair(mtu: usize) -> (Link, UnixDatagram) {
        let (ours, theirs) = UnixDatagram::pair().unwrap();
        (Link { fd: OwnedFd::from(ours), mtu, peer: [0; 6] }, theirs)
    }

    #[test]
    fn a_packet_beyond_the_mtu_is_read_whole() {
        let (link, phone) = pair(48);
        let sent = [7u8; 120];
        assert_eq!(phone.send(&sent).unwrap(), 120);
        let mut packet = [0u8; MAX_PACKET];
        assert_eq!(link.read(&mut packet).unwrap(), 120);
        assert_eq!(packet[..120], sent);
    }

    #[test]
    fn audio_goes_out_in_pieces_of_the_mtu() {
        let (link, phone) = pair(48);
        assert_eq!(link.send(&[1u8; 120]), 120);
        let mut piece = [0u8; MAX_PACKET];
        let sizes: Vec<usize> = (0..3).map(|_| phone.recv(&mut piece).unwrap()).collect();
        assert_eq!(sizes, [48, 48, 24]);
    }
}
