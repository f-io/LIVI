// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::CStr;
use std::io;
use std::net::Ipv6Addr;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use super::{
    FRAME_BUF, Link, NcmBridge, find_ncm_function, format_mac, host_mac, open_usb, read_within,
    write_all,
};
use crate::Mac;
use crate::android::open;
use crate::ether::{FromPhone, Ip};

const PREFIX: u8 = 64;
const MTU: u16 = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunRequest {
    pub address: Ipv6Addr,
    pub prefix: u8,
    pub mtu: u16,
}

pub type OpenTun = Arc<dyn Fn(&TunRequest) -> Result<OwnedFd, String> + Send + Sync>;

struct Tun {
    fd: OwnedFd,
    ip: Ip,
}

impl Link for Tun {
    fn deliver(&self, frame: &[u8]) -> io::Result<Option<Vec<u8>>> {
        match self.ip.incoming(frame) {
            FromPhone::Deliver(packet) => write_all(&self.fd, packet).map(|()| None),
            FromPhone::Answer(answer) => Ok(Some(answer)),
            FromPhone::Ignore => Ok(None),
        }
    }

    fn next(&self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; FRAME_BUF];
        Ok(read_within(&self.fd, &mut buf)?.and_then(|n| self.ip.outgoing(&buf[..n])))
    }
}

fn made_up_mac(serial: &str) -> Mac {
    let mut mac = [0x02, 0, 0, 0, 0, 0];
    for (i, b) in serial.bytes().enumerate() {
        mac[1 + i % 5] ^= b;
    }
    mac
}

fn iface_with(address: Ipv6Addr) -> Option<String> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return None;
    }
    let mut found = None;
    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        if i32::from(unsafe { (*ifa.ifa_addr).sa_family }) != libc::AF_INET6 {
            continue;
        }
        let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
        if Ipv6Addr::from(sin6.sin6_addr.s6_addr) == address {
            found = unsafe { CStr::from_ptr(ifa.ifa_name) }
                .to_str()
                .ok()
                .map(str::to_string);
            break;
        }
    }
    unsafe { libc::freeifaddrs(ifap) };
    found
}

impl NcmBridge {
    pub fn start(serial: &str, open_tun: &OpenTun) -> Result<Self, String> {
        let device = open(serial)?;
        let func = find_ncm_function(&device).ok_or("no NCM function in the active config")?;
        let usb = open_usb(&device, &func)?;
        let mac = host_mac(&device, &func).unwrap_or_else(|| made_up_mac(serial));
        let ip = Ip::new(mac);
        let fd = open_tun(&TunRequest {
            address: ip.address(),
            prefix: PREFIX,
            mtu: MTU,
        })?;
        let ifname = iface_with(ip.address())
            .ok_or_else(|| format!("no interface carries {}", ip.address()))?;
        println!(
            "[ncm] up on {ifname}: if{}/{} ep=0x{:02x}/0x{:02x} mac={} ip={}",
            func.control,
            func.data,
            func.ep_in,
            func.ep_out,
            format_mac(&mac),
            ip.address()
        );
        let address = ip.address();
        let mut bridge = Self::up(ifname, usb, Tun { fd, ip });
        bridge.address = Some(address);
        Ok(bridge)
    }
}
