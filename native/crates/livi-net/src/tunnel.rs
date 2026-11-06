// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{self, Read};
#[cfg(target_os = "linux")]
use std::net::TcpStream;
#[cfg(target_os = "linux")]
use std::time::Duration;

pub const PACKET_MAX: usize = 4096;

#[cfg(target_os = "linux")]
pub fn notice_loss(stream: &TcpStream, give_up: Option<Duration>) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let mut options = vec![
        (libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 2),
    ];
    if let Some(after) = give_up {
        let ms = libc::c_int::try_from(after.as_millis()).unwrap_or(libc::c_int::MAX);
        options.push((libc::IPPROTO_TCP, libc::TCP_USER_TIMEOUT, ms));
    }
    let mut failed = None;
    for (level, name, value) in options {
        let value: libc::c_int = value;
        let rc = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                level,
                name,
                (&raw const value).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 && failed.is_none() {
            failed = Some(io::Error::last_os_error());
        }
    }
    failed.map_or(Ok(()), Err)
}

pub fn frame(pkt: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + pkt.len());
    out.extend_from_slice(&(pkt.len() as u16).to_be_bytes());
    out.extend_from_slice(pkt);
    out
}

pub fn receive(input: &mut impl Read, buf: &mut [u8; PACKET_MAX]) -> io::Result<usize> {
    let mut len = [0u8; 2];
    input.read_exact(&mut len)?;
    let n = usize::from(u16::from_be_bytes(len));
    if n == 0 || n > PACKET_MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {n} bytes"),
        ));
    }
    input.read_exact(&mut buf[..n])?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_comes_back_as_it_was_framed() {
        let mut wire = frame(&[0x04, 0x0e, 0x01]);
        wire.extend(frame(&[0x01]));
        let mut input = wire.as_slice();
        let mut buf = [0u8; PACKET_MAX];
        assert_eq!(receive(&mut input, &mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], &[0x04, 0x0e, 0x01]);
        assert_eq!(receive(&mut input, &mut buf).unwrap(), 1);
        assert_eq!(
            receive(&mut input, &mut buf).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn an_empty_or_oversized_frame_ends_the_tunnel() {
        let mut buf = [0u8; PACKET_MAX];
        let empty = [0u8, 0];
        assert_eq!(
            receive(&mut empty.as_slice(), &mut buf).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let huge = ((PACKET_MAX + 1) as u16).to_be_bytes();
        assert_eq!(
            receive(&mut huge.as_slice(), &mut buf).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
