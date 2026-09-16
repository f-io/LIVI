//! mDNS wire format for a single name. We answer for one host with the address of the interface
//! the query came in on, and say via NSEC that only an A record exists.

use std::net::Ipv4Addr;

pub const PORT: u16 = 5353;
pub const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const TTL: u32 = 120;
/// A legacy (unicast, non-5353) asker gets a short TTL and no cache-flush bit.
const LEGACY_TTL: u32 = 10;

const QTYPE_A: u16 = 1;
const QTYPE_AAAA: u16 = 28;
const QTYPE_NSEC: u16 = 47;
const QTYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
/// Set on an answer so listeners replace what they cached.
const CACHE_FLUSH: u16 = 0x8000;
/// Set by an asker that wants the reply to itself rather than to the group.
const UNICAST_REPLY: u16 = 0x8000;

/// `<host>.local` as DNS labels.
pub struct Name(Vec<u8>);

impl Name {
    pub fn new(host: &str) -> Self {
        let mut wire = Vec::new();
        for label in [host, "local"] {
            let bytes = &label.as_bytes()[..label.len().min(63)];
            wire.push(bytes.len() as u8);
            wire.extend_from_slice(bytes);
        }
        wire.push(0);
        Self(wire)
    }

    fn matches(&self, other: &[u8]) -> bool {
        self.0.len() == other.len()
            && self
                .0
                .iter()
                .zip(other)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }
}

/// What a query wants from us.
#[derive(Debug, PartialEq, Eq)]
pub struct Ask {
    pub id: u16,
    pub want_a: bool,
    /// The name was asked for AAAA: answer NSEC, which says there is none.
    pub want_nsec: bool,
    pub unicast: bool,
}

/// Reads the questions and reports what concerns our name.
pub fn parse_query(pkt: &[u8], name: &Name) -> Option<Ask> {
    if pkt.len() < 12 || pkt[2] & 0x80 != 0 {
        return None; // too short, or a response rather than a query
    }
    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    let questions = u16::from_be_bytes([pkt[4], pkt[5]]);
    let mut ask = Ask {
        id,
        want_a: false,
        want_nsec: false,
        unicast: false,
    };
    let mut pos = 12;
    for _ in 0..questions {
        let Some((qname, next)) = read_name(pkt, pos) else {
            break;
        };
        pos = next;
        if pos + 4 > pkt.len() {
            break;
        }
        let qtype = u16::from_be_bytes([pkt[pos], pkt[pos + 1]]);
        let qclass = u16::from_be_bytes([pkt[pos + 2], pkt[pos + 3]]);
        pos += 4;
        if qclass & !UNICAST_REPLY != CLASS_IN || !name.matches(&qname) {
            continue;
        }
        ask.want_a |= qtype == QTYPE_A || qtype == QTYPE_ANY;
        ask.want_nsec |= qtype == QTYPE_AAAA || qtype == QTYPE_ANY;
        if ask.want_a || ask.want_nsec {
            ask.unicast |= qclass & UNICAST_REPLY != 0;
        }
    }
    (ask.want_a || ask.want_nsec).then_some(ask)
}

/// The answer: an A record, and an NSEC record saying A is all there is.
pub fn build_answer(
    name: &Name,
    id: u16,
    addr: Ipv4Addr,
    legacy: bool,
    with_a: bool,
    with_nsec: bool,
) -> Vec<u8> {
    let class = if legacy {
        CLASS_IN
    } else {
        CLASS_IN | CACHE_FLUSH
    };
    let ttl = if legacy { LEGACY_TTL } else { TTL };
    let mut b = Vec::with_capacity(128);
    b.extend_from_slice(&id.to_be_bytes());
    b.extend_from_slice(&[0x84, 0x00]); // response, authoritative
    b.extend_from_slice(&0u16.to_be_bytes()); // questions
    b.extend_from_slice(&(u16::from(with_a) + u16::from(with_nsec)).to_be_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]); // no authority, no additional
    if with_a {
        put_rr_head(&mut b, name, QTYPE_A, class, ttl);
        b.extend_from_slice(&4u16.to_be_bytes());
        b.extend_from_slice(&addr.octets());
    }
    if with_nsec {
        put_rr_head(&mut b, name, QTYPE_NSEC, class, ttl);
        // next name = itself, then one bitmap window: window 0, one byte, bit for type A
        b.extend_from_slice(&((name.0.len() + 3) as u16).to_be_bytes());
        b.extend_from_slice(&name.0);
        b.extend_from_slice(&[0, 1, 0x40]);
    }
    b
}

fn put_rr_head(b: &mut Vec<u8>, name: &Name, rtype: u16, class: u16, ttl: u32) {
    b.extend_from_slice(&name.0);
    b.extend_from_slice(&rtype.to_be_bytes());
    b.extend_from_slice(&class.to_be_bytes());
    b.extend_from_slice(&ttl.to_be_bytes());
}

/// Expands the name at `pos`, following compression pointers, and reports where the name ends
/// in the packet (which is not where it ends after a jump).
fn read_name(pkt: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let (mut i, mut end, mut hops) = (pos, None, 0);
    let mut out = Vec::new();
    loop {
        let len = *pkt.get(i)?;
        if len & 0xC0 == 0xC0 {
            let next = *pkt.get(i + 1)?;
            hops += 1;
            if hops > 8 {
                return None; // a pointer loop
            }
            end.get_or_insert(i + 2);
            i = usize::from(len & 0x3F) << 8 | usize::from(next);
            continue;
        }
        let len = usize::from(len);
        if out.len() + 1 + len >= 255 || i + 1 + len > pkt.len() {
            return None;
        }
        out.push(len as u8);
        if len == 0 {
            break;
        }
        out.extend_from_slice(&pkt[i + 1..i + 1 + len]);
        i += 1 + len;
    }
    Some((out, end.unwrap_or(i + 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(qtype: u16, qclass: u16, host: &str) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&Name::new(host).0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&qclass.to_be_bytes());
        q
    }

    #[test]
    fn answers_an_a_query_for_our_name() {
        let name = Name::new("livi-link");
        let ask = parse_query(&query(QTYPE_A, CLASS_IN, "livi-link"), &name).unwrap();
        assert_eq!(
            ask,
            Ask {
                id: 0x1234,
                want_a: true,
                want_nsec: false,
                unicast: false
            }
        );
    }

    #[test]
    fn ignores_other_names_responses_and_classes() {
        let name = Name::new("livi-link");
        assert!(parse_query(&query(QTYPE_A, CLASS_IN, "someone-else"), &name).is_none());
        assert!(parse_query(&query(QTYPE_A, 3, "livi-link"), &name).is_none());
        let mut response = query(QTYPE_A, CLASS_IN, "livi-link");
        response[2] = 0x84;
        assert!(parse_query(&response, &name).is_none());
    }

    #[test]
    fn an_aaaa_query_gets_nsec_not_an_address() {
        let name = Name::new("livi-link");
        let ask = parse_query(&query(QTYPE_AAAA, CLASS_IN, "livi-link"), &name).unwrap();
        assert!(!ask.want_a && ask.want_nsec);
    }

    #[test]
    fn honours_the_unicast_reply_bit() {
        let name = Name::new("livi-link");
        let ask = parse_query(
            &query(QTYPE_ANY, CLASS_IN | UNICAST_REPLY, "livi-link"),
            &name,
        )
        .unwrap();
        assert!(ask.want_a && ask.want_nsec && ask.unicast);
    }

    #[test]
    fn follows_a_compression_pointer() {
        let name = Name::new("livi-link");
        // Question name is a pointer to the name laid out at the end of the packet.
        let mut q = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        let target = 12 + 2 + 4;
        q.extend_from_slice(&[0xC0, target as u8]);
        q.extend_from_slice(&QTYPE_A.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q.extend_from_slice(&Name::new("livi-link").0);
        assert!(parse_query(&q, &name).unwrap().want_a);
    }

    #[test]
    fn a_pointer_loop_is_not_followed_forever() {
        let name = Name::new("livi-link");
        let mut q = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&[0xC0, 12]); // points at itself
        q.extend_from_slice(&QTYPE_A.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        assert!(parse_query(&q, &name).is_none());
    }

    #[test]
    fn the_multicast_answer_carries_the_cache_flush_bit_and_the_address() {
        let name = Name::new("livi-link");
        let a = Ipv4Addr::new(192, 168, 50, 2);
        let answer = build_answer(&name, 0, a, false, true, true);
        assert_eq!(&answer[..4], &[0, 0, 0x84, 0x00]);
        assert_eq!(u16::from_be_bytes([answer[6], answer[7]]), 2); // A + NSEC
        let rr = 12 + name.0.len();
        assert_eq!(u16::from_be_bytes([answer[rr], answer[rr + 1]]), QTYPE_A);
        assert_eq!(
            u16::from_be_bytes([answer[rr + 2], answer[rr + 3]]),
            CLASS_IN | CACHE_FLUSH
        );
        assert!(answer.windows(4).any(|w| w == a.octets()));
    }

    #[test]
    fn a_legacy_answer_keeps_the_id_and_stays_out_of_caches() {
        let name = Name::new("livi-link");
        let answer = build_answer(&name, 0x1234, Ipv4Addr::LOCALHOST, true, true, false);
        assert_eq!(u16::from_be_bytes([answer[0], answer[1]]), 0x1234);
        let rr = 12 + name.0.len();
        assert_eq!(
            u16::from_be_bytes([answer[rr + 2], answer[rr + 3]]),
            CLASS_IN
        );
        assert_eq!(
            u32::from_be_bytes(answer[rr + 4..rr + 8].try_into().unwrap()),
            LEGACY_TTL
        );
    }
}
