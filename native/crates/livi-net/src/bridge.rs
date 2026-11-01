// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::Path;

pub const USB: &str = "usb0";

pub fn from_usb(listener: &TcpListener) -> impl Iterator<Item = TcpStream> + '_ {
    let mut last = None;
    listener.incoming().flatten().filter(move |stream| {
        let (Ok(peer), Ok(local)) = (stream.peer_addr(), stream.local_addr()) else {
            return false;
        };
        if over_usb(Path::new("/"), peer.ip(), local.ip()) {
            return true;
        }
        if last.replace(peer.ip()) != Some(peer.ip()) {
            println!(
                "[link] :{} turned away {}, not on the USB link",
                local.port(),
                peer.ip()
            );
        }
        false
    })
}

fn over_usb(root: &Path, peer: IpAddr, local: IpAddr) -> bool {
    if peer.is_loopback() || peer == local {
        return true;
    }
    let arp = fs::read_to_string(root.join("proc/net/arp")).unwrap_or_default();
    let Some((mac, dev)) = neighbour(&arp, peer) else {
        return false;
    };
    if dev == USB {
        return true;
    }
    let Some(usb) = port_no(root, USB) else {
        return false;
    };
    entries(&fdb(root, dev)).any(|e| e.mac == mac && e.port == usb)
}

fn neighbour(arp: &str, ip: IpAddr) -> Option<([u8; 6], &str)> {
    arp.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [addr, _, _, mac, _, dev] = fields[..] else {
            return None;
        };
        if addr.parse::<IpAddr>().ok() != Some(ip) {
            return None;
        }
        Some((parse_mac(mac)?, dev))
    })
}

fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let mut mac = [0; 6];
    let mut parts = text.split(':');
    for byte in &mut mac {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    (parts.next().is_none() && mac != [0; 6]).then_some(mac)
}

fn port_no(root: &Path, iface: &str) -> Option<u16> {
    let text =
        fs::read_to_string(root.join(format!("sys/class/net/{iface}/brport/port_no"))).ok()?;
    u16::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

fn fdb(root: &Path, bridge: &str) -> Vec<u8> {
    fs::read(root.join(format!("sys/class/net/{bridge}/brforward"))).unwrap_or_default()
}

struct Entry {
    mac: [u8; 6],
    port: u16,
}

fn entries(fdb: &[u8]) -> impl Iterator<Item = Entry> + '_ {
    fdb.as_chunks::<16>().0.iter().map(|e| Entry {
        mac: [e[0], e[1], e[2], e[3], e[4], e[5]],
        port: u16::from(e[6]) | u16::from(e[12]) << 8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const HOST: [u8; 6] = [0x06, 0x7c, 0x56, 0xf6, 0xa8, 0xec];
    const STATION: [u8; 6] = [0x1e, 0x8e, 0x05, 0x8b, 0x09, 0xad];
    const OWN: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const ARP: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
10.10.10.103     0x1         0x2         1e:8e:05:8b:09:ad     *        br0
10.10.10.100     0x1         0x2         06:7c:56:f6:a8:ec     *        br0
10.10.10.102     0x1         0x0         00:00:00:00:00:00     *        br0
";

    fn entry(mac: [u8; 6], port: u16, local: bool) -> [u8; 16] {
        let mut e = [0; 16];
        e[..6].copy_from_slice(&mac);
        e[6] = port as u8;
        e[7] = local.into();
        e[12] = (port >> 8) as u8;
        e
    }

    fn dongle(name: &str, arp: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("livi-net-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (iface, port) in [(USB, "0x1"), ("wlan0", "0x2")] {
            let dir = root.join(format!("sys/class/net/{iface}/brport"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("port_no"), format!("{port}\n")).unwrap();
        }
        fs::create_dir_all(root.join("proc/net")).unwrap();
        fs::write(root.join("proc/net/arp"), arp).unwrap();
        let fdb = [
            entry(HOST, 1, false),
            entry(STATION, 2, false),
            entry(OWN, 1, true),
        ]
        .concat();
        fs::create_dir_all(root.join("sys/class/net/br0")).unwrap();
        fs::write(root.join("sys/class/net/br0/brforward"), fdb).unwrap();
        root
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn the_host_behind_usb_gets_in_and_a_wifi_client_does_not() {
        let root = dongle("ports", ARP);
        let own = ip("10.10.10.1");
        assert!(over_usb(&root, ip("10.10.10.100"), own));
        assert!(!over_usb(&root, ip("10.10.10.103"), own));
        assert!(
            !over_usb(&root, ip("10.10.10.102"), own),
            "no answer yet, no MAC"
        );
        assert!(!over_usb(&root, ip("10.10.10.150"), own), "never seen");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn this_device_and_a_usb_link_outside_a_bridge_get_in() {
        let root = dongle(
            "self",
            "header\n10.10.10.100 0x1 0x2 06:7c:56:f6:a8:ec * usb0\n",
        );
        assert!(over_usb(&root, ip("127.0.0.1"), ip("127.0.0.1")));
        assert!(over_usb(&root, ip("10.10.10.1"), ip("10.10.10.1")));
        assert!(over_usb(&root, ip("10.10.10.100"), ip("10.10.10.1")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_port_past_255_keeps_its_high_byte() {
        let e = entries(&entry(HOST, 0x102, false)).next().unwrap();
        assert_eq!((e.mac, e.port), (HOST, 0x102));
    }

    #[test]
    fn a_mac_needs_six_bytes_that_are_not_all_zero() {
        assert_eq!(parse_mac("06:7c:56:f6:a8:ec"), Some(HOST));
        assert_eq!(parse_mac("00:00:00:00:00:00"), None);
        assert_eq!(parse_mac("06:7c:56:f6:a8"), None);
        assert_eq!(parse_mac("06:7c:56:f6:a8:ec:00"), None);
    }

    #[test]
    fn a_connection_from_this_device_comes_through() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        assert!(from_usb(&listener).next().is_some());
    }
}
