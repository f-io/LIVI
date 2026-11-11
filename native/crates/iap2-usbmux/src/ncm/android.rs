// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::CStr;
use std::io;
use std::net::Ipv6Addr;
use std::sync::Arc;

use super::tunnel::{Port, Tunnel};
use super::{Link, NcmBridge, find_ncm_function, format_mac, host_mac, open_usb};
use crate::android::open;
use crate::ether::{FromPhone, Ip};
use crate::{Error, Mac};

struct Tun {
    port: Arc<Port>,
    ip: Ip,
}

impl Link for Tun {
    fn deliver(&self, frame: &[u8]) -> io::Result<Option<Vec<u8>>> {
        match self.ip.incoming(frame) {
            FromPhone::Deliver(packet) => self.port.write(packet).map(|()| None),
            FromPhone::Answer(answer) => Ok(Some(answer)),
            FromPhone::Ignore => Ok(None),
        }
    }

    fn next(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self
            .port
            .recv()
            .and_then(|packet| self.ip.outgoing(&packet)))
    }
}

fn made_up_mac(serial: &str) -> Mac {
    let mut mac = [0x02, 0, 0, 0, 0, 0];
    for (i, b) in serial.bytes().enumerate() {
        mac[1 + i % 5] ^= b;
    }
    mac
}

fn ipv6_addresses() -> Vec<(String, Ipv6Addr)> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: ifap is a valid out pointer, freed below.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut cur = ifap;
    while !cur.is_null() {
        // SAFETY: cur is a non-null node of the list getifaddrs returned, alive until freeifaddrs.
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        // SAFETY: ifa_addr was checked for null and points into the list.
        if i32::from(unsafe { (*ifa.ifa_addr).sa_family }) != libc::AF_INET6 {
            continue;
        }
        // SAFETY: an AF_INET6 address is a sockaddr_in6.
        let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
        // SAFETY: ifa_name was checked for null and is nul-terminated.
        if let Ok(label) = unsafe { CStr::from_ptr(ifa.ifa_name) }.to_str() {
            // bionic names IPv6 entries after the interface's IPv4 label ("tun0:1")
            let name = label.split(':').next().unwrap_or(label);
            found.push((name.to_string(), Ipv6Addr::from(sin6.sin6_addr.s6_addr)));
        }
    }
    // SAFETY: ifap came from getifaddrs and no reference into it outlives this line.
    unsafe { libc::freeifaddrs(ifap) };
    found
}

pub(super) fn iface_with(address: Ipv6Addr) -> Option<String> {
    ipv6_addresses()
        .into_iter()
        .find_map(|(name, a)| (a == address).then_some(name))
}

pub(super) fn ipv6_on(ifname: &str) -> Vec<Ipv6Addr> {
    ipv6_addresses()
        .into_iter()
        .filter_map(|(name, a)| (name == ifname).then_some(a))
        .collect()
}

impl NcmBridge {
    pub fn over<L: Link>(
        device: &nusb::Device,
        serial: &str,
        ifname: &str,
        link: impl FnOnce(Mac) -> L,
    ) -> Result<Self, Error> {
        let func = find_ncm_function(device).ok_or(Error::NoNcmFunction)?;
        let usb = open_usb(device, &func)?;
        let link = link(host_mac(device, &func).unwrap_or_else(|| made_up_mac(serial)));
        Ok(Self::up(ifname.to_string(), usb, link))
    }

    pub fn start(serial: &str, tunnel: &Arc<Tunnel>) -> Result<Self, Error> {
        let device = open(serial)?;
        let func = find_ncm_function(&device).ok_or(Error::NoNcmFunction)?;
        let usb = open_usb(&device, &func)?;
        let mac = host_mac(&device, &func).unwrap_or_else(|| made_up_mac(serial));
        let port = tunnel.join(None)?;
        let ip = Ip::with_address(mac, port.address());
        let ifname = port
            .ifname()
            .ok_or_else(|| Error::NoInterface(ip.address()))?;
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
        let mut bridge = Self::up(ifname, usb, Tun { port, ip });
        bridge.address = Some(address);
        Ok(bridge)
    }
}
