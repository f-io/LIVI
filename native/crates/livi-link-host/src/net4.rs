// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::net::Ipv4Addr;

pub type Mac = [u8; 6];

const ETH_HEADER: usize = 14;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_ARP: u16 = 0x0806;
const BROADCAST: Mac = [0xff; 6];
const IPV4_HEADER: usize = 20;
const UDP_HEADER: usize = 8;
const PROTO_UDP: u8 = 17;
const TTL: u8 = 64;
const DHCP_CLIENT: u16 = 68;
const DHCP_SERVER: u16 = 67;
const BOOTP_LEN: usize = 236;
const BOOTP_REQUEST: u8 = 1;
const BOOTP_REPLY: u8 = 2;
const HTYPE_ETHERNET: u8 = 1;
const BROADCAST_FLAG: u16 = 0x8000;
const MAGIC: [u8; 4] = [99, 130, 83, 99];
const OPT_PAD: u8 = 0;
const OPT_MASK: u8 = 1;
const OPT_REQUESTED: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_TYPE: u8 = 53;
const OPT_SERVER: u8 = 54;
const OPT_PARAMS: u8 = 55;
const OPT_END: u8 = 255;
const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;
const ARP_LEN: usize = 28;
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;
const DEFAULT_PREFIX: u8 = 24;
const DEFAULT_LEASE_S: u32 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub server: Ipv4Addr,
    pub seconds: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Incoming<'a> {
    Deliver(&'a [u8]),
    Answer(Vec<u8>),
    Leased(Lease),
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outgoing {
    Send(Vec<u8>),
    Resolve(Vec<u8>),
    Drop,
}

pub struct Net4 {
    mac: Mac,
    xid: u32,
    offered: Option<Lease>,
    lease: Option<Lease>,
    neighbours: HashMap<Ipv4Addr, Mac>,
}

impl Net4 {
    pub fn new(mac: Mac, xid: u32) -> Self {
        Self {
            mac,
            xid,
            offered: None,
            lease: None,
            neighbours: HashMap::new(),
        }
    }

    pub fn lease(&self) -> Option<Lease> {
        self.lease
    }

    pub fn ask(&self) -> Vec<u8> {
        match self.lease.or(self.offered) {
            Some(lease) => dhcp(self.mac, self.xid, REQUEST, Some(lease)),
            None => dhcp(self.mac, self.xid, DISCOVER, None),
        }
    }

    pub fn incoming<'a>(&mut self, frame: &'a [u8]) -> Incoming<'a> {
        if frame.len() < ETH_HEADER {
            return Incoming::Ignore;
        }
        let source: Mac = frame[6..12].try_into().unwrap_or_default();
        let payload = &frame[ETH_HEADER..];
        match u16::from_be_bytes([frame[12], frame[13]]) {
            ETHERTYPE_ARP => self.arp(payload),
            ETHERTYPE_IPV4 if payload.len() >= IPV4_HEADER => {
                if let Some(answer) = dhcp_reply(payload, self.mac, self.xid) {
                    return self.answered(answer);
                }
                let from = Ipv4Addr::from(<[u8; 4]>::try_from(&payload[12..16]).unwrap());
                if source[0] & 1 == 0 && self.on_subnet(from) {
                    self.neighbours.insert(from, source);
                }
                if self.lease.is_some() {
                    Incoming::Deliver(payload)
                } else {
                    Incoming::Ignore
                }
            }
            _ => Incoming::Ignore,
        }
    }

    pub fn outgoing(&self, packet: &[u8]) -> Outgoing {
        let Some(lease) = self.lease else {
            return Outgoing::Drop;
        };
        if packet.len() < IPV4_HEADER || packet[0] >> 4 != 4 {
            return Outgoing::Drop;
        }
        let dest = Ipv4Addr::from(<[u8; 4]>::try_from(&packet[16..20]).unwrap());
        let mac = if dest.is_multicast() {
            let o = dest.octets();
            [0x01, 0x00, 0x5e, o[1] & 0x7f, o[2], o[3]]
        } else if dest.is_broadcast() || dest == subnet_broadcast(lease) {
            BROADCAST
        } else if let Some(mac) = self.neighbours.get(&dest) {
            *mac
        } else {
            return Outgoing::Resolve(arp_frame(
                self.mac,
                BROADCAST,
                ARP_REQUEST,
                (self.mac, lease.address),
                ([0; 6], dest),
            ));
        };
        Outgoing::Send(ethernet(mac, self.mac, ETHERTYPE_IPV4, packet))
    }

    fn answered(&mut self, answer: Dhcp) -> Incoming<'static> {
        match answer {
            Dhcp::Offer(lease) if self.lease.is_none() => {
                self.offered = Some(lease);
                Incoming::Answer(dhcp(self.mac, self.xid, REQUEST, Some(lease)))
            }
            Dhcp::Ack(lease) => match self.lease {
                Some(held) if held.address != lease.address => Incoming::Ignore,
                Some(_) => {
                    self.lease = Some(lease);
                    Incoming::Ignore
                }
                None => {
                    self.lease = Some(lease);
                    Incoming::Leased(lease)
                }
            },
            Dhcp::Nak if self.lease.is_none() => {
                self.offered = None;
                Incoming::Ignore
            }
            _ => Incoming::Ignore,
        }
    }

    fn arp(&mut self, payload: &[u8]) -> Incoming<'static> {
        let Some(arp) = Arp::parse(payload) else {
            return Incoming::Ignore;
        };
        let Some(lease) = self.lease else {
            return Incoming::Ignore;
        };
        if self.on_subnet(arp.sender_ip) && !arp.sender_ip.is_unspecified() {
            self.neighbours.insert(arp.sender_ip, arp.sender_mac);
        }
        if arp.op == ARP_REQUEST && arp.target_ip == lease.address {
            return Incoming::Answer(arp_frame(
                self.mac,
                arp.sender_mac,
                ARP_REPLY,
                (self.mac, lease.address),
                (arp.sender_mac, arp.sender_ip),
            ));
        }
        Incoming::Ignore
    }

    fn on_subnet(&self, ip: Ipv4Addr) -> bool {
        let lease = self.lease.or(self.offered);
        lease.is_some_and(|l| mask(ip, l.prefix) == mask(l.address, l.prefix))
    }
}

fn mask(ip: Ipv4Addr, prefix: u8) -> u32 {
    let bits = u32::from(ip);
    match prefix {
        0 => 0,
        p if p >= 32 => bits,
        p => bits & (u32::MAX << (32 - p)),
    }
}

fn subnet_broadcast(lease: Lease) -> Ipv4Addr {
    let host = if lease.prefix >= 32 {
        0
    } else {
        u32::MAX >> lease.prefix
    };
    Ipv4Addr::from(mask(lease.address, lease.prefix) | host)
}

fn ethernet(dest: Mac, source: Mac, ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(ETH_HEADER + payload.len());
    frame.extend_from_slice(&dest);
    frame.extend_from_slice(&source);
    frame.extend_from_slice(&ethertype.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn udp(source: Ipv4Addr, dest: Ipv4Addr, ports: (u16, u16), payload: &[u8]) -> Vec<u8> {
    let total = IPV4_HEADER + UDP_HEADER + payload.len();
    let mut packet = Vec::with_capacity(total);
    packet.extend_from_slice(&[0x45, 0]);
    packet.extend_from_slice(&(total as u16).to_be_bytes());
    packet.extend_from_slice(&[0, 0, 0, 0, TTL, PROTO_UDP, 0, 0]);
    packet.extend_from_slice(&source.octets());
    packet.extend_from_slice(&dest.octets());
    let sum = livi_base::checksum::inet(&[&packet[..IPV4_HEADER]]);
    packet[10..12].copy_from_slice(&sum.to_be_bytes());
    packet.extend_from_slice(&ports.0.to_be_bytes());
    packet.extend_from_slice(&ports.1.to_be_bytes());
    packet.extend_from_slice(&((UDP_HEADER + payload.len()) as u16).to_be_bytes());
    packet.extend_from_slice(&[0, 0]);
    packet.extend_from_slice(payload);
    packet
}

fn dhcp(mac: Mac, xid: u32, kind: u8, lease: Option<Lease>) -> Vec<u8> {
    let mut bootp = vec![0u8; BOOTP_LEN];
    bootp[0] = BOOTP_REQUEST;
    bootp[1] = HTYPE_ETHERNET;
    bootp[2] = 6;
    bootp[4..8].copy_from_slice(&xid.to_be_bytes());
    bootp[10..12].copy_from_slice(&BROADCAST_FLAG.to_be_bytes());
    bootp[28..34].copy_from_slice(&mac);
    bootp.extend_from_slice(&MAGIC);
    bootp.extend_from_slice(&[OPT_TYPE, 1, kind]);
    if let Some(lease) = lease {
        bootp.extend_from_slice(&[OPT_REQUESTED, 4]);
        bootp.extend_from_slice(&lease.address.octets());
        bootp.extend_from_slice(&[OPT_SERVER, 4]);
        bootp.extend_from_slice(&lease.server.octets());
    }
    bootp.extend_from_slice(&[OPT_PARAMS, 2, OPT_MASK, OPT_LEASE, OPT_END]);
    let packet = udp(
        Ipv4Addr::UNSPECIFIED,
        Ipv4Addr::BROADCAST,
        (DHCP_CLIENT, DHCP_SERVER),
        &bootp,
    );
    ethernet(BROADCAST, mac, ETHERTYPE_IPV4, &packet)
}

#[derive(Debug, PartialEq, Eq)]
enum Dhcp {
    Offer(Lease),
    Ack(Lease),
    Nak,
}

fn dhcp_reply(packet: &[u8], mac: Mac, xid: u32) -> Option<Dhcp> {
    let header = usize::from(packet[0] & 0x0f) * 4;
    if packet[9] != PROTO_UDP || packet.len() < header + UDP_HEADER {
        return None;
    }
    let udp = &packet[header..];
    if u16::from_be_bytes([udp[2], udp[3]]) != DHCP_CLIENT {
        return None;
    }
    let bootp = udp.get(UDP_HEADER..)?;
    if bootp.len() < BOOTP_LEN + MAGIC.len()
        || bootp[0] != BOOTP_REPLY
        || bootp[4..8] != xid.to_be_bytes()
        || bootp[28..34] != mac
        || bootp[BOOTP_LEN..BOOTP_LEN + 4] != MAGIC
    {
        return None;
    }
    let address = Ipv4Addr::from(<[u8; 4]>::try_from(&bootp[16..20]).ok()?);
    let (mut kind, mut prefix, mut server, mut seconds) = (0, DEFAULT_PREFIX, None, None);
    let mut options = &bootp[BOOTP_LEN + 4..];
    while let [code, rest @ ..] = options {
        match *code {
            OPT_END => break,
            OPT_PAD => {
                options = rest;
                continue;
            }
            _ => {}
        }
        let [len, rest @ ..] = rest else { break };
        let len = usize::from(*len);
        let value = rest.get(..len)?;
        match (*code, value) {
            (OPT_TYPE, [k]) => kind = *k,
            (OPT_MASK, [a, b, c, d]) => {
                prefix = u32::from_be_bytes([*a, *b, *c, *d]).count_ones() as u8;
            }
            (OPT_SERVER, [a, b, c, d]) => server = Some(Ipv4Addr::new(*a, *b, *c, *d)),
            (OPT_LEASE, [a, b, c, d]) => seconds = Some(u32::from_be_bytes([*a, *b, *c, *d])),
            _ => {}
        }
        options = &rest[len..];
    }
    let from = Ipv4Addr::from(<[u8; 4]>::try_from(&packet[12..16]).ok()?);
    let lease = Lease {
        address,
        prefix,
        server: server.unwrap_or(from),
        seconds: seconds.unwrap_or(DEFAULT_LEASE_S),
    };
    match kind {
        OFFER => Some(Dhcp::Offer(lease)),
        ACK => Some(Dhcp::Ack(lease)),
        NAK => Some(Dhcp::Nak),
        _ => None,
    }
}

struct Arp {
    op: u16,
    sender_mac: Mac,
    sender_ip: Ipv4Addr,
    target_ip: Ipv4Addr,
}

impl Arp {
    fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < ARP_LEN || payload[0..6] != [0, 1, 8, 0, 6, 4] {
            return None;
        }
        Some(Self {
            op: u16::from_be_bytes([payload[6], payload[7]]),
            sender_mac: payload[8..14].try_into().ok()?,
            sender_ip: Ipv4Addr::from(<[u8; 4]>::try_from(&payload[14..18]).ok()?),
            target_ip: Ipv4Addr::from(<[u8; 4]>::try_from(&payload[24..28]).ok()?),
        })
    }
}

fn arp_frame(
    ours: Mac,
    dest: Mac,
    op: u16,
    sender: (Mac, Ipv4Addr),
    target: (Mac, Ipv4Addr),
) -> Vec<u8> {
    let mut arp = vec![0, 1, 8, 0, 6, 4];
    arp.extend_from_slice(&op.to_be_bytes());
    arp.extend_from_slice(&sender.0);
    arp.extend_from_slice(&sender.1.octets());
    arp.extend_from_slice(&target.0);
    arp.extend_from_slice(&target.1.octets());
    ethernet(dest, ours, ETHERTYPE_ARP, &arp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: Mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    const DONGLE: Mac = [0x0e, 0x6a, 0xc4, 0x63, 0x0d, 0xd2];
    const PHONE: Mac = [0x36, 0xab, 0xcd, 0xef, 0x01, 0x23];
    const XID: u32 = 0x1234_5678;

    fn server() -> Ipv4Addr {
        Ipv4Addr::new(10, 10, 10, 1)
    }

    fn reply(kind: u8, address: Ipv4Addr, xid: u32) -> Vec<u8> {
        let mut bootp = vec![0u8; BOOTP_LEN];
        bootp[0] = BOOTP_REPLY;
        bootp[1] = HTYPE_ETHERNET;
        bootp[2] = 6;
        bootp[4..8].copy_from_slice(&xid.to_be_bytes());
        bootp[16..20].copy_from_slice(&address.octets());
        bootp[28..34].copy_from_slice(&OURS);
        bootp.extend_from_slice(&MAGIC);
        bootp.extend_from_slice(&[OPT_TYPE, 1, kind, OPT_PAD]);
        bootp.extend_from_slice(&[OPT_MASK, 4, 255, 255, 255, 0]);
        bootp.extend_from_slice(&[OPT_SERVER, 4, 10, 10, 10, 1]);
        bootp.extend_from_slice(&[OPT_LEASE, 4, 0, 1, 0x51, 0x80, OPT_END]);
        let packet = udp(
            server(),
            Ipv4Addr::BROADCAST,
            (DHCP_SERVER, DHCP_CLIENT),
            &bootp,
        );
        ethernet(BROADCAST, DONGLE, ETHERTYPE_IPV4, &packet)
    }

    fn leased() -> Net4 {
        let mut net = Net4::new(OURS, XID);
        let address = Ipv4Addr::new(10, 10, 10, 100);
        let _ = net.incoming(&reply(OFFER, address, XID));
        let _ = net.incoming(&reply(ACK, address, XID));
        net
    }

    fn tcp(source: Ipv4Addr, dest: Ipv4Addr) -> Vec<u8> {
        let mut p = vec![0x45, 0, 0, 40, 0, 0, 0, 0, 64, 6, 0, 0];
        p.extend_from_slice(&source.octets());
        p.extend_from_slice(&dest.octets());
        p.extend_from_slice(&[0; 20]);
        p
    }

    #[test]
    fn the_first_ask_is_a_broadcast_discover() {
        let frame = Net4::new(OURS, XID).ask();
        assert_eq!(&frame[0..6], &BROADCAST);
        assert_eq!(&frame[6..12], &OURS);
        let packet = &frame[ETH_HEADER..];
        assert_eq!(livi_base::checksum::inet(&[&packet[..IPV4_HEADER]]), 0);
        assert_eq!(&packet[16..20], &[255; 4]);
        let bootp = &packet[IPV4_HEADER + UDP_HEADER..];
        assert_eq!(&bootp[4..8], &XID.to_be_bytes());
        assert_eq!(&bootp[28..34], &OURS);
        assert_eq!(
            &bootp[BOOTP_LEN + 4..BOOTP_LEN + 7],
            &[OPT_TYPE, 1, DISCOVER]
        );
    }

    #[test]
    fn an_offer_is_requested_and_the_ack_leases_the_address() {
        let mut net = Net4::new(OURS, XID);
        let address = Ipv4Addr::new(10, 10, 10, 100);
        let Incoming::Answer(request) = net.incoming(&reply(OFFER, address, XID)) else {
            panic!("no request");
        };
        let bootp = &request[ETH_HEADER + IPV4_HEADER + UDP_HEADER..];
        assert_eq!(
            &bootp[BOOTP_LEN + 4..BOOTP_LEN + 7],
            &[OPT_TYPE, 1, REQUEST]
        );
        assert_eq!(&bootp[BOOTP_LEN + 9..BOOTP_LEN + 13], &address.octets());
        let lease = Lease {
            address,
            prefix: 24,
            server: server(),
            seconds: 86400,
        };
        assert_eq!(
            net.incoming(&reply(ACK, address, XID)),
            Incoming::Leased(lease)
        );
        assert_eq!(net.lease(), Some(lease));
        assert_eq!(net.incoming(&reply(ACK, address, XID)), Incoming::Ignore);
    }

    #[test]
    fn replies_for_someone_else_are_ignored() {
        let mut net = Net4::new(OURS, XID);
        let address = Ipv4Addr::new(10, 10, 10, 100);
        assert_eq!(
            net.incoming(&reply(OFFER, address, XID + 1)),
            Incoming::Ignore
        );
        assert_eq!(net.lease(), None);
    }

    #[test]
    fn a_request_for_our_address_is_answered() {
        let mut net = leased();
        let ask = arp_frame(
            DONGLE,
            BROADCAST,
            ARP_REQUEST,
            (DONGLE, server()),
            ([0; 6], Ipv4Addr::new(10, 10, 10, 100)),
        );
        let Incoming::Answer(answer) = net.incoming(&ask) else {
            panic!("no answer");
        };
        assert_eq!(&answer[0..6], &DONGLE);
        let arp = Arp::parse(&answer[ETH_HEADER..]).unwrap();
        assert_eq!(arp.op, ARP_REPLY);
        assert_eq!(arp.sender_mac, OURS);
        assert_eq!(arp.sender_ip, Ipv4Addr::new(10, 10, 10, 100));
    }

    #[test]
    fn packets_go_to_the_mac_that_spoke_for_their_address() {
        let mut net = leased();
        let phone = Ipv4Addr::new(10, 10, 10, 120);
        let ours = Ipv4Addr::new(10, 10, 10, 100);
        let Outgoing::Resolve(ask) = net.outgoing(&tcp(ours, phone)) else {
            panic!("no resolve");
        };
        assert_eq!(Arp::parse(&ask[ETH_HEADER..]).unwrap().target_ip, phone);
        let hello = tcp(phone, ours);
        let frame = ethernet(OURS, PHONE, ETHERTYPE_IPV4, &hello);
        assert_eq!(net.incoming(&frame), Incoming::Deliver(hello.as_slice()));
        let Outgoing::Send(back) = net.outgoing(&tcp(ours, phone)) else {
            panic!("not sent");
        };
        assert_eq!(&back[0..6], &PHONE);
        assert_eq!(&back[6..12], &OURS);
    }

    #[test]
    fn broadcast_and_multicast_need_no_neighbour() {
        let net = leased();
        let ours = Ipv4Addr::new(10, 10, 10, 100);
        let Outgoing::Send(mdns) = net.outgoing(&tcp(ours, Ipv4Addr::new(224, 0, 0, 251))) else {
            panic!("not sent");
        };
        assert_eq!(&mdns[0..6], &[0x01, 0x00, 0x5e, 0, 0, 251]);
        let Outgoing::Send(all) = net.outgoing(&tcp(ours, Ipv4Addr::new(10, 10, 10, 255))) else {
            panic!("not sent");
        };
        assert_eq!(&all[0..6], &BROADCAST);
    }

    #[test]
    fn nothing_flows_before_the_lease() {
        let mut net = Net4::new(OURS, XID);
        let ours = Ipv4Addr::new(10, 10, 10, 100);
        assert_eq!(net.outgoing(&tcp(ours, server())), Outgoing::Drop);
        let hello = tcp(server(), ours);
        let frame = ethernet(OURS, DONGLE, ETHERTYPE_IPV4, &hello);
        assert_eq!(net.incoming(&frame), Incoming::Ignore);
    }
}
