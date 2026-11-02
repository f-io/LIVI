// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::Ipv6Addr;

use crate::wire::{
    CACHE_FLUSH, CLASS_IN, Name, QTYPE_AAAA, QTYPE_ANY, UNICAST_REPLY, put_rr, read_name,
};

const QTYPE_PTR: u16 = 12;
const QTYPE_TXT: u16 = 16;
const QTYPE_SRV: u16 = 33;
const SHARED_TTL: u32 = 4500;
const UNIQUE_TTL: u32 = 120;

pub struct Service {
    kind: Name,
    instance: Name,
    host: Name,
    port: u16,
    txt: Vec<u8>,
    address: Ipv6Addr,
}

impl Service {
    pub fn new(
        instance: &str,
        kind: &str,
        host: &str,
        port: u16,
        txt: &[String],
        address: Ipv6Addr,
    ) -> Self {
        let mut labels: Vec<&str> = kind.split('.').collect();
        labels.push("local");
        let kind_wire = Name::of(&labels);
        labels.insert(0, instance);
        let mut packed = Vec::new();
        for entry in txt {
            let bytes = &entry.as_bytes()[..entry.len().min(255)];
            packed.push(bytes.len() as u8);
            packed.extend_from_slice(bytes);
        }
        Self {
            kind: kind_wire,
            instance: Name::of(&labels),
            host: Name::new(host),
            port,
            txt: packed,
            address,
        }
    }

    pub fn asked(&self, pkt: &[u8]) -> Option<bool> {
        if pkt.len() < 12 || pkt[2] & 0x80 != 0 {
            return None;
        }
        let questions = u16::from_be_bytes([pkt[4], pkt[5]]);
        let (mut pos, mut hit, mut unicast) = (12, false, false);
        for _ in 0..questions {
            let (name, next) = read_name(pkt, pos)?;
            pos = next;
            let head = pkt.get(pos..pos + 4)?;
            pos += 4;
            let qtype = u16::from_be_bytes([head[0], head[1]]);
            let qclass = u16::from_be_bytes([head[2], head[3]]);
            if qclass & !UNICAST_REPLY != CLASS_IN {
                continue;
            }
            let wanted = match qtype {
                QTYPE_PTR => self.kind.matches(&name),
                QTYPE_SRV | QTYPE_TXT => self.instance.matches(&name),
                QTYPE_AAAA => self.host.matches(&name),
                QTYPE_ANY => [&self.kind, &self.instance, &self.host]
                    .iter()
                    .any(|n| n.matches(&name)),
                _ => false,
            };
            if wanted {
                hit = true;
                unicast |= qclass & UNICAST_REPLY != 0;
            }
        }
        hit.then_some(unicast)
    }

    pub fn records(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(512);
        b.extend_from_slice(&[0, 0, 0x84, 0x00, 0, 0, 0, 3, 0, 0, 0, 1]);
        let shared = CLASS_IN;
        put_rr(
            &mut b,
            &self.kind,
            QTYPE_PTR,
            shared,
            SHARED_TTL,
            &self.instance.0,
        );
        let mut srv = vec![0, 0, 0, 0];
        srv.extend_from_slice(&self.port.to_be_bytes());
        srv.extend_from_slice(&self.host.0);
        let unique = CLASS_IN | CACHE_FLUSH;
        put_rr(&mut b, &self.instance, QTYPE_SRV, unique, UNIQUE_TTL, &srv);
        put_rr(
            &mut b,
            &self.instance,
            QTYPE_TXT,
            unique,
            UNIQUE_TTL,
            &self.txt,
        );
        let address = self.address.octets();
        put_rr(&mut b, &self.host, QTYPE_AAAA, unique, UNIQUE_TTL, &address);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Service {
        Service::new(
            "LIVI",
            "_airplay._tcp",
            "LIVI",
            7000,
            &[
                "deviceid=D8:3A:DD:E5:66:B9".to_string(),
                "model=LIVI".to_string(),
            ],
            "fe80::c6a:c4ff:fe63:dd2".parse().unwrap(),
        )
    }

    fn query(name: &[&str], qtype: u16, qclass: u16) -> Vec<u8> {
        let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&Name::of(name).0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&qclass.to_be_bytes());
        q
    }

    #[test]
    fn a_browse_for_the_service_type_is_answered() {
        let q = query(&["_airplay", "_tcp", "local"], QTYPE_PTR, CLASS_IN);
        assert_eq!(service().asked(&q), Some(false));
    }

    #[test]
    fn the_instance_and_the_host_are_resolved() {
        let s = service();
        let srv = query(&["LIVI", "_airplay", "_tcp", "local"], QTYPE_SRV, CLASS_IN);
        assert_eq!(s.asked(&srv), Some(false));
        let aaaa = query(&["livi", "local"], QTYPE_AAAA, CLASS_IN | UNICAST_REPLY);
        assert_eq!(s.asked(&aaaa), Some(true));
    }

    #[test]
    fn other_services_and_responses_are_left_alone() {
        let s = service();
        assert_eq!(
            s.asked(&query(&["_hap", "_tcp", "local"], QTYPE_PTR, CLASS_IN)),
            None
        );
        let mut response = query(&["_airplay", "_tcp", "local"], QTYPE_PTR, CLASS_IN);
        response[2] = 0x84;
        assert_eq!(s.asked(&response), None);
    }

    #[test]
    fn the_records_carry_the_port_the_txt_and_the_address() {
        let s = service();
        let r = s.records();
        assert_eq!(&r[2..4], &[0x84, 0x00]);
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 3);
        assert_eq!(u16::from_be_bytes([r[10], r[11]]), 1);
        assert!(r.windows(2).any(|w| w == 7000u16.to_be_bytes()));
        assert!(r.windows(10).any(|w| w == b"model=LIVI"));
        assert!(r.ends_with(&s.address.octets()));
    }
}
