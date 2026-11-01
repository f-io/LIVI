// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

use livi_bt_host::sdp::{CARPLAY, IAP, PDU_MAX, answer};
pub use livi_bt_host::sdp::{CARPLAY_UUID, IAP_CLIENT_UUID, IAP_UUID, Record};
const BTPROTO_L2CAP: libc::c_int = 0;
const PSM: u16 = 1;

pub const ADVERTISED: [[u8; 16]; 3] = [IAP_UUID, IAP_CLIENT_UUID, CARPLAY_UUID];

pub static RECORDS: [&Record; 2] = [&IAP, &CARPLAY];
static OFFERED: AtomicBool = AtomicBool::new(false);

pub fn set_offered(on: bool) -> bool {
    OFFERED.swap(on, Ordering::Relaxed) != on
}

pub fn offered() -> bool {
    OFFERED.load(Ordering::Relaxed)
}

fn on_offer() -> Vec<&'static Record> {
    if offered() {
        RECORDS.to_vec()
    } else {
        Vec::new()
    }
}

#[repr(C)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

pub fn serve() -> Result<(), String> {
    let listener = listen()?;
    for r in &RECORDS {
        println!("[sdp] offering {} on channel {}", r.name, r.channel);
    }
    loop {
        let raw = unsafe {
            libc::accept(
                listener.as_raw_fd(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if raw < 0 {
            return Err(format!("accept: {}", std::io::Error::last_os_error()));
        }
        let client = unsafe { OwnedFd::from_raw_fd(raw) };
        converse(client);
    }
}

fn listen() -> Result<OwnedFd, String> {
    let raw = unsafe {
        libc::socket(
            libc::AF_BLUETOOTH,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            BTPROTO_L2CAP,
        )
    };
    if raw < 0 {
        return Err(format!("socket: {}", std::io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrL2 {
        family: libc::AF_BLUETOOTH as libc::sa_family_t,
        psm: PSM.to_le(),
        bdaddr: [0; 6],
        cid: 0,
        bdaddr_type: 0,
    };
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            size_of::<SockaddrL2>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!(
            "bind psm {PSM}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::listen(fd.as_raw_fd(), 4) } < 0 {
        return Err(format!("listen: {}", std::io::Error::last_os_error()));
    }
    Ok(fd)
}

fn converse(client: OwnedFd) {
    let mut sock = std::fs::File::from(client);
    let mut buf = [0u8; PDU_MAX];
    loop {
        let Ok(n) = sock.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        let reply = answer(&buf[..n], &on_offer());
        if sock.write_all(&reply).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_records_are_listed_only_while_a_host_takes_them() {
        let found = || {
            let mut search = vec![0x02, 0x00, 0x01, 0x00, 0x13, 0x35, 0x11, 0x1c];
            search.extend_from_slice(&CARPLAY_UUID);
            answer(&search, &on_offer())
                .windows(4)
                .any(|w| w == RECORDS[1].handle.to_be_bytes())
        };
        assert!(!found());
        assert!(set_offered(true));
        assert!(!set_offered(true));
        assert!(found());
        assert!(set_offered(false));
        assert!(!found());
    }
}
