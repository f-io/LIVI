// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::Ipv6Addr;
use std::sync::Mutex;

use crate::Mac;

const ETH_HEADER: usize = 14;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_IPV6: u16 = 0x86dd;
const IPV6_HEADER: usize = 40;
const NEXT_ICMPV6: u8 = 58;
const NEIGHBOR_SOLICITATION: u8 = 135;
const NEIGHBOR_ADVERTISEMENT: u8 = 136;
const NA_SOLICITED_OVERRIDE: u8 = 0x60;
const OPTION_SOURCE_LINK_ADDRESS: u8 = 1;
const OPTION_TARGET_LINK_ADDRESS: u8 = 2;
const NS_LEN: usize = 24;
const NA_LEN: usize = 32;
const HOP_LIMIT_NDP: u8 = 255;
const BROADCAST: Mac = [0xff; 6];

pub fn link_local(mac: Mac) -> Ipv6Addr {
    Ipv6Addr::from([
        0xfe,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xff,
        0xfe,
        mac[3],
        mac[4],
        mac[5],
    ])
}

#[derive(Debug, PartialEq, Eq)]
pub enum FromPhone<'a> {
    Deliver(&'a [u8]),
    Answer(Vec<u8>),
    Ignore,
}

pub struct Ip {
    mac: Mac,
    address: Ipv6Addr,
    phone: Mutex<Option<Mac>>,
}

impl Ip {
    pub fn new(mac: Mac) -> Self {
        Self {
            mac,
            address: link_local(mac),
            phone: Mutex::new(None),
        }
    }

    pub fn address(&self) -> Ipv6Addr {
        self.address
    }

    pub fn incoming<'a>(&self, frame: &'a [u8]) -> FromPhone<'a> {
        if frame.len() < ETH_HEADER {
            return FromPhone::Ignore;
        }
        let source: Mac = frame[6..12].try_into().unwrap_or_default();
        if source[0] & 1 == 0 {
            *self.phone.lock().unwrap() = Some(source);
        }
        let packet = &frame[ETH_HEADER..];
        match u16::from_be_bytes([frame[12], frame[13]]) {
            ETHERTYPE_IPV6 if neighbor_solicitation(packet) => match solicited(packet) {
                Some((asker, target)) => {
                    FromPhone::Answer(self.advertisement(source, asker, target))
                }
                None => FromPhone::Ignore,
            },
            ETHERTYPE_IPV6 if packet.len() >= IPV6_HEADER => FromPhone::Deliver(packet),
            ETHERTYPE_IPV4 => FromPhone::Deliver(packet),
            _ => FromPhone::Ignore,
        }
    }

    pub fn outgoing(&self, packet: &[u8]) -> Option<Vec<u8>> {
        let (ethertype, dest) = match packet.first()? >> 4 {
            6 if packet.len() >= IPV6_HEADER => {
                let dest = &packet[24..40];
                let mac = if dest[0] == 0xff {
                    [0x33, 0x33, dest[12], dest[13], dest[14], dest[15]]
                } else {
                    self.phone()
                };
                (ETHERTYPE_IPV6, mac)
            }
            4 if packet.len() >= 20 => {
                let dest = &packet[16..20];
                let mac = if dest[0] & 0xf0 == 0xe0 {
                    [0x01, 0x00, 0x5e, dest[1] & 0x7f, dest[2], dest[3]]
                } else {
                    self.phone()
                };
                (ETHERTYPE_IPV4, mac)
            }
            _ => return None,
        };
        let mut frame = Vec::with_capacity(ETH_HEADER + packet.len());
        frame.extend_from_slice(&dest);
        frame.extend_from_slice(&self.mac);
        frame.extend_from_slice(&ethertype.to_be_bytes());
        frame.extend_from_slice(packet);
        Some(frame)
    }

    fn phone(&self) -> Mac {
        self.phone.lock().unwrap().unwrap_or(BROADCAST)
    }

    fn advertisement(&self, mac: Mac, to: Ipv6Addr, target: Ipv6Addr) -> Vec<u8> {
        let mut icmp = [0u8; NA_LEN];
        icmp[0] = NEIGHBOR_ADVERTISEMENT;
        icmp[4] = NA_SOLICITED_OVERRIDE;
        icmp[8..24].copy_from_slice(&target.octets());
        icmp[24] = OPTION_TARGET_LINK_ADDRESS;
        icmp[25] = 1;
        icmp[26..32].copy_from_slice(&self.mac);
        let sum = checksum(target, to, &icmp);
        icmp[2..4].copy_from_slice(&sum.to_be_bytes());

        let mut frame = Vec::with_capacity(ETH_HEADER + IPV6_HEADER + NA_LEN);
        frame.extend_from_slice(&mac);
        frame.extend_from_slice(&self.mac);
        frame.extend_from_slice(&ETHERTYPE_IPV6.to_be_bytes());
        frame.extend_from_slice(&[0x60, 0, 0, 0]);
        frame.extend_from_slice(&(NA_LEN as u16).to_be_bytes());
        frame.extend_from_slice(&[NEXT_ICMPV6, HOP_LIMIT_NDP]);
        frame.extend_from_slice(&target.octets());
        frame.extend_from_slice(&to.octets());
        frame.extend_from_slice(&icmp);
        frame
    }
}

fn neighbor_solicitation(packet: &[u8]) -> bool {
    packet.len() >= IPV6_HEADER + NS_LEN
        && packet[6] == NEXT_ICMPV6
        && packet[IPV6_HEADER] == NEIGHBOR_SOLICITATION
}

fn solicited(packet: &[u8]) -> Option<(Ipv6Addr, Ipv6Addr)> {
    let icmp = packet.get(IPV6_HEADER..)?;
    let source = Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?);
    let dest = Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?);
    let target = Ipv6Addr::from(<[u8; 16]>::try_from(&icmp[8..24]).ok()?);
    let reachable = !dest.is_multicast() || source_link_address(&icmp[NS_LEN..]);
    (!source.is_unspecified() && target != source && reachable).then_some((source, target))
}

fn source_link_address(mut options: &[u8]) -> bool {
    while let [kind, len, ..] = *options {
        let size = usize::from(len) * 8;
        if size == 0 || size > options.len() {
            return false;
        }
        if kind == OPTION_SOURCE_LINK_ADDRESS {
            return true;
        }
        options = &options[size..];
    }
    false
}

fn checksum(source: Ipv6Addr, dest: Ipv6Addr, icmp: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut add = |bytes: &[u8]| {
        for pair in bytes.chunks(2) {
            sum += u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]));
        }
    };
    add(&source.octets());
    add(&dest.octets());
    add(&(icmp.len() as u32).to_be_bytes());
    add(&[0, 0, 0, NEXT_ICMPV6]);
    add(icmp);
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: Mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    const PHONE: Mac = [0x36, 0xab, 0xcd, 0xef, 0x01, 0x23];

    fn ipv6(next: u8, source: Ipv6Addr, dest: Ipv6Addr, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![0x60, 0, 0, 0];
        p.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        p.extend_from_slice(&[next, 255]);
        p.extend_from_slice(&source.octets());
        p.extend_from_slice(&dest.octets());
        p.extend_from_slice(payload);
        p
    }

    fn frame(dest: Mac, source: Mac, ethertype: u16, packet: &[u8]) -> Vec<u8> {
        let mut f = dest.to_vec();
        f.extend_from_slice(&source);
        f.extend_from_slice(&ethertype.to_be_bytes());
        f.extend_from_slice(packet);
        f
    }

    fn solicitation(target: Ipv6Addr) -> Vec<u8> {
        let mut ns = vec![NEIGHBOR_SOLICITATION, 0, 0, 0, 0, 0, 0, 0];
        ns.extend_from_slice(&target.octets());
        ns
    }

    fn phone_ip() -> Ipv6Addr {
        link_local(PHONE)
    }

    #[test]
    fn the_address_follows_the_mac() {
        assert_eq!(
            link_local(OURS),
            "fe80::11:22ff:fe33:4455".parse::<Ipv6Addr>().unwrap()
        );
    }

    #[test]
    fn a_solicitation_for_us_is_answered_with_our_mac() {
        let ip = Ip::new(OURS);
        let ns = ipv6(
            NEXT_ICMPV6,
            phone_ip(),
            ip.address(),
            &solicitation(ip.address()),
        );
        let FromPhone::Answer(na) = ip.incoming(&frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &ns))
        else {
            panic!("no answer");
        };
        assert_eq!(&na[0..6], &PHONE);
        assert_eq!(&na[6..12], &OURS);
        let packet = &na[ETH_HEADER..];
        assert_eq!(&packet[8..24], &ip.address().octets());
        assert_eq!(&packet[24..40], &phone_ip().octets());
        let icmp = &packet[IPV6_HEADER..];
        assert_eq!(icmp[0], NEIGHBOR_ADVERTISEMENT);
        assert_eq!(&icmp[26..32], &OURS);
        assert_eq!(checksum(ip.address(), phone_ip(), icmp), 0);
    }

    #[test]
    fn an_address_the_kernel_added_is_answered_too() {
        let ip = Ip::new(OURS);
        let other: Ipv6Addr = "fe80::2465:8a71:a6f4:9a77".parse().unwrap();
        let ns = ipv6(NEXT_ICMPV6, phone_ip(), other, &solicitation(other));
        let FromPhone::Answer(na) = ip.incoming(&frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &ns))
        else {
            panic!("no answer");
        };
        let packet = &na[ETH_HEADER..];
        assert_eq!(&packet[8..24], &other.octets());
        assert_eq!(&packet[IPV6_HEADER + 8..IPV6_HEADER + 24], &other.octets());
        assert_eq!(checksum(other, phone_ip(), &packet[IPV6_HEADER..]), 0);
    }

    #[test]
    fn other_solicitations_and_duplicate_checks_stay_unanswered() {
        let ip = Ip::new(OURS);
        let other = ipv6(
            NEXT_ICMPV6,
            phone_ip(),
            phone_ip(),
            &solicitation(phone_ip()),
        );
        let f = frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &other);
        assert_eq!(ip.incoming(&f), FromPhone::Ignore);
        let dad = ipv6(
            NEXT_ICMPV6,
            Ipv6Addr::UNSPECIFIED,
            ip.address(),
            &solicitation(ip.address()),
        );
        let f = frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &dad);
        assert_eq!(ip.incoming(&f), FromPhone::Ignore);
    }

    #[test]
    fn a_multicast_solicitation_needs_the_askers_link_address() {
        let ip = Ip::new(OURS);
        let group: Ipv6Addr = "ff02::1:ff63:dd2".parse().unwrap();
        let optimistic = ipv6(NEXT_ICMPV6, phone_ip(), group, &solicitation(ip.address()));
        let f = frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &optimistic);
        assert_eq!(ip.incoming(&f), FromPhone::Ignore);
        let mut ns = solicitation(ip.address());
        ns.extend_from_slice(&[OPTION_SOURCE_LINK_ADDRESS, 1]);
        ns.extend_from_slice(&PHONE);
        let preferred = ipv6(NEXT_ICMPV6, phone_ip(), group, &ns);
        let f = frame(BROADCAST, PHONE, ETHERTYPE_IPV6, &preferred);
        assert!(matches!(ip.incoming(&f), FromPhone::Answer(_)));
    }

    #[test]
    fn packets_go_back_to_the_mac_the_phone_spoke_from() {
        let ip = Ip::new(OURS);
        let tcp = ipv6(6, ip.address(), phone_ip(), &[0; 20]);
        assert_eq!(&ip.outgoing(&tcp).unwrap()[0..6], &BROADCAST);
        let hello = ipv6(6, phone_ip(), ip.address(), &[0; 20]);
        let f = frame(OURS, PHONE, ETHERTYPE_IPV6, &hello);
        assert_eq!(ip.incoming(&f), FromPhone::Deliver(hello.as_slice()));
        let back = ip.outgoing(&tcp).unwrap();
        assert_eq!(&back[0..6], &PHONE);
        assert_eq!(&back[6..12], &OURS);
        assert_eq!(&back[12..14], &ETHERTYPE_IPV6.to_be_bytes());
        assert_eq!(&back[ETH_HEADER..], tcp.as_slice());
    }

    #[test]
    fn multicast_goes_to_its_group_mac() {
        let ip = Ip::new(OURS);
        let mdns: Ipv6Addr = "ff02::fb".parse().unwrap();
        let packet = ipv6(17, ip.address(), mdns, &[0; 8]);
        assert_eq!(
            &ip.outgoing(&packet).unwrap()[0..6],
            &[0x33, 0x33, 0, 0, 0, 0xfb]
        );
        let mut v4 = vec![0x45; 20];
        v4[16..20].copy_from_slice(&[224, 0, 0, 251]);
        assert_eq!(
            &ip.outgoing(&v4).unwrap()[0..6],
            &[0x01, 0x00, 0x5e, 0, 0, 251]
        );
    }

    #[test]
    fn foreign_frames_are_ignored() {
        let ip = Ip::new(OURS);
        assert_eq!(ip.incoming(&[0; 10]), FromPhone::Ignore);
        let arp = frame(BROADCAST, PHONE, 0x0806, &[0; 28]);
        assert_eq!(ip.incoming(&arp), FromPhone::Ignore);
        assert_eq!(ip.outgoing(&[]), None);
        assert_eq!(ip.outgoing(&[0x10; 40]), None);
    }
}
