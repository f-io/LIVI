//! The service records we answer for, and the one question we ask a phone: which RFCOMM channel
//! a service of it sits on.

pub const PDU_MAX: usize = 1024;

/// Wireless iAP.
pub const IAP_UUID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0xde, 0xca, 0xfa, 0xde, 0xde, 0xca, 0xde, 0xaf, 0xde, 0xca, 0xca, 0xff,
];
/// Wireless iAP the other way round, offered by the phone.
pub const IAP_CLIENT_UUID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0xde, 0xca, 0xfa, 0xde, 0xde, 0xca, 0xde, 0xaf, 0xde, 0xca, 0xca, 0xfe,
];
pub const CARPLAY_UUID: [u8; 16] = [
    0xec, 0x88, 0x43, 0x48, 0xcd, 0x41, 0x40, 0xa2, 0x97, 0x27, 0x57, 0x5d, 0x50, 0xbf, 0x1f, 0xd3,
];
pub const ANDROID_AUTO_UUID: [u8; 16] = [
    0x4d, 0xe1, 0x7a, 0x00, 0x52, 0xcb, 0x11, 0xe6, 0xbd, 0xf4, 0x08, 0x00, 0x20, 0x0c, 0x9a, 0x66,
];
pub const HANDS_FREE_GATEWAY: u16 = 0x111f;
const HANDS_FREE: u16 = 0x111e;
const GENERIC_AUDIO: u16 = 0x1203;
const HANDS_FREE_VERSION: u16 = 0x0108;
const HANDS_FREE_FEATURES: u16 = 0x009c;

pub const fn full_uuid(id: u16) -> [u8; 16] {
    let [hi, lo] = id.to_be_bytes();
    [0, 0, hi, lo, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b, 0x34, 0xfb]
}

#[derive(Clone, Copy, PartialEq)]
pub enum Profile {
    SerialPort,
    HandsFree,
}

pub struct Record {
    pub handle: u32,
    pub uuid: [u8; 16],
    pub channel: u8,
    pub name: &'static str,
    pub profile: Profile,
}

pub static IAP: Record = Record {
    handle: 0x0001_0001,
    uuid: IAP_UUID,
    channel: 3,
    name: "Wireless iAP",
    profile: Profile::SerialPort,
};
pub static CARPLAY: Record = Record {
    handle: 0x0001_0002,
    uuid: CARPLAY_UUID,
    channel: 4,
    name: "CarPlay",
    profile: Profile::SerialPort,
};
pub static ANDROID_AUTO: Record = Record {
    handle: 0x0001_0003,
    uuid: ANDROID_AUTO_UUID,
    channel: 8,
    name: "Android Auto Wireless",
    profile: Profile::SerialPort,
};
/// The phone connects here after pairing, and only then opens Android Auto.
pub static HANDS_FREE_UNIT: Record = Record {
    handle: 0x0001_0004,
    uuid: full_uuid(HANDS_FREE),
    channel: 7,
    name: "HFP Hands-Free",
    profile: Profile::HandsFree,
};

/// A phone that finds a service's record uses it, so each is listed only while it is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    CarPlay,
    HandsFree,
    AndroidAuto,
}

impl Service {
    pub const ALL: [Service; 3] = [Service::CarPlay, Service::HandsFree, Service::AndroidAuto];

    pub fn records(self) -> Vec<&'static Record> {
        match self {
            Service::CarPlay => vec![&IAP, &CARPLAY],
            Service::HandsFree => vec![&HANDS_FREE_UNIT],
            Service::AndroidAuto => vec![&ANDROID_AUTO],
        }
    }

    /// What the name list a phone sees before connecting carries.
    pub fn uuids(self) -> Vec<[u8; 16]> {
        match self {
            Service::CarPlay => vec![IAP_UUID, IAP_CLIENT_UUID, CARPLAY_UUID],
            Service::HandsFree => vec![HANDS_FREE_UNIT.uuid],
            Service::AndroidAuto => vec![ANDROID_AUTO_UUID],
        }
    }

    pub fn of_channel(channel: u8) -> Option<Service> {
        Service::ALL.into_iter().find(|s| s.records().iter().any(|r| r.channel == channel))
    }
}

const UUID_L2CAP: u16 = 0x0100;
const UUID_RFCOMM: u16 = 0x0003;
const UUID_BROWSE_ROOT: u16 = 0x1002;
const UUID_SERIAL_PORT: u16 = 0x1101;

const REQ_SERVICE_SEARCH: u8 = 0x02;
const REQ_ATTRIBUTE: u8 = 0x04;
const REQ_SEARCH_ATTRIBUTE: u8 = 0x06;
const RSP_SERVICE_SEARCH: u8 = 0x03;
const RSP_ATTRIBUTE: u8 = 0x05;
const RSP_SEARCH_ATTRIBUTE: u8 = 0x07;
const RSP_ERROR: u8 = 0x01;
const ERROR_SYNTAX: u16 = 0x0003;
const ATTR_PROTOCOLS: u16 = 0x0004;

/// One request from a phone, answered from the records on offer.
pub fn answer(req: &[u8], offered: &[&Record]) -> Vec<u8> {
    if req.len() < 5 {
        return error(0, ERROR_SYNTAX);
    }
    let pdu = req[0];
    let tid = u16::from_be_bytes([req[1], req[2]]);
    let body = &req[5..];
    match pdu {
        REQ_SEARCH_ATTRIBUTE => search_attribute(tid, body, offered),
        REQ_ATTRIBUTE => attribute(tid, body, offered),
        REQ_SERVICE_SEARCH => service_search(tid, body, offered),
        _ => error(tid, ERROR_SYNTAX),
    }
}

/// Asks for the protocol list of every record carrying the service.
pub fn ask_channel(tid: u16, service: &[u8]) -> Vec<u8> {
    let mut params = seq(service);
    params.extend_from_slice(&u16::MAX.to_be_bytes());
    params.extend_from_slice(&seq(&uint16(ATTR_PROTOCOLS)));
    params.push(0);
    packet(REQ_SEARCH_ATTRIBUTE, tid, &params)
}

/// The channel is the uint8 right behind the RFCOMM UUID in the answer.
pub fn channel_in(reply: &[u8]) -> Option<u8> {
    if reply.first() != Some(&RSP_SEARCH_ATTRIBUTE) {
        return None;
    }
    reply.windows(5).find(|w| w[..4] == [0x19, 0x00, 0x03, 0x08]).map(|w| w[4])
}

pub fn uuid_of(id: u16) -> Vec<u8> {
    uuid16(id)
}

pub fn uuid_of_128(id: &[u8; 16]) -> Vec<u8> {
    uuid128(id)
}

fn search_attribute(tid: u16, body: &[u8], offered: &[&Record]) -> Vec<u8> {
    let Some((pattern, rest)) = element(body) else {
        return error(tid, ERROR_SYNTAX);
    };
    if rest.len() < 2 {
        return error(tid, ERROR_SYNTAX);
    }
    let max = usize::from(u16::from_be_bytes([rest[0], rest[1]]));
    let Some((attrs, rest)) = element(&rest[2..]) else {
        return error(tid, ERROR_SYNTAX);
    };
    let start = continuation(rest);
    let mut found = Vec::new();
    for r in offered.iter().filter(|r| wanted(pattern, r)) {
        found.extend_from_slice(&record(r, attrs));
    }
    let lists = seq(&found);
    let (chunk, next) = slice(&lists, start, max);
    let mut params = Vec::new();
    params.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
    params.extend_from_slice(chunk);
    params.extend_from_slice(&next);
    packet(RSP_SEARCH_ATTRIBUTE, tid, &params)
}

fn attribute(tid: u16, body: &[u8], offered: &[&Record]) -> Vec<u8> {
    if body.len() < 6 {
        return error(tid, ERROR_SYNTAX);
    }
    let handle = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let max = usize::from(u16::from_be_bytes([body[4], body[5]]));
    let Some((attrs, rest)) = element(&body[6..]) else {
        return error(tid, ERROR_SYNTAX);
    };
    let start = continuation(rest);
    let list = match offered.iter().find(|r| r.handle == handle) {
        Some(r) => record(r, attrs),
        None => seq(&[]),
    };
    let (chunk, next) = slice(&list, start, max);
    let mut params = Vec::new();
    params.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
    params.extend_from_slice(chunk);
    params.extend_from_slice(&next);
    packet(RSP_ATTRIBUTE, tid, &params)
}

fn service_search(tid: u16, body: &[u8], offered: &[&Record]) -> Vec<u8> {
    let Some((pattern, _)) = element(body) else {
        return error(tid, ERROR_SYNTAX);
    };
    let hits: Vec<&&Record> = offered.iter().filter(|r| wanted(pattern, r)).collect();
    let count = hits.len() as u16;
    let mut params = Vec::new();
    params.extend_from_slice(&count.to_be_bytes());
    params.extend_from_slice(&count.to_be_bytes());
    for r in hits {
        params.extend_from_slice(&r.handle.to_be_bytes());
    }
    params.push(0);
    packet(RSP_SERVICE_SEARCH, tid, &params)
}

fn wanted(pattern: &[u8], record: &Record) -> bool {
    let mut rest = pattern;
    while let Some((value, next)) = element(rest) {
        let hit = match value.len() {
            2 => {
                let id = u16::from_be_bytes([value[0], value[1]]);
                id == UUID_BROWSE_ROOT
                    || id == UUID_L2CAP
                    || id == UUID_RFCOMM
                    || record.uuid == full_uuid(id)
            }
            16 => value == record.uuid,
            _ => false,
        };
        if hit {
            return true;
        }
        rest = next;
    }
    false
}

fn record(record: &Record, attrs: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, value) in attributes(record) {
        if asked(attrs, id) {
            out.extend_from_slice(&uint16(id));
            out.extend_from_slice(&value);
        }
    }
    seq(&out)
}

/// Attribute ids have to stay in ascending order.
fn attributes(record: &Record) -> Vec<(u16, Vec<u8>)> {
    let protocol = seq(&[
        seq(&uuid16(UUID_L2CAP)),
        seq(&[uuid16(UUID_RFCOMM), uint8(record.channel)].concat()),
    ]
    .concat());
    let (class, profile) = match record.profile {
        Profile::SerialPort => (
            seq(&uuid128(&record.uuid)),
            seq(&seq(&[uuid16(UUID_SERIAL_PORT), uint16(0x0100)].concat())),
        ),
        Profile::HandsFree => (
            seq(&[uuid16(HANDS_FREE), uuid16(GENERIC_AUDIO)].concat()),
            seq(&seq(&[uuid16(HANDS_FREE), uint16(HANDS_FREE_VERSION)].concat())),
        ),
    };
    let mut out = vec![
        (0x0000, uint32(record.handle)),
        (0x0001, class),
        (0x0002, uint32(0)),
        (ATTR_PROTOCOLS, protocol),
        (0x0005, seq(&uuid16(UUID_BROWSE_ROOT))),
        (0x0008, uint8(0xff)),
        (0x0009, profile),
        (0x0100, text(record.name)),
    ];
    if record.profile == Profile::HandsFree {
        out.push((0x0311, uint16(HANDS_FREE_FEATURES)));
    }
    out
}

fn asked(attrs: &[u8], id: u16) -> bool {
    let mut rest = attrs;
    while let Some((value, next)) = element(rest) {
        match value.len() {
            2 if u16::from_be_bytes([value[0], value[1]]) == id => return true,
            4 => {
                let first = u16::from_be_bytes([value[0], value[1]]);
                let last = u16::from_be_bytes([value[2], value[3]]);
                if (first..=last).contains(&id) {
                    return true;
                }
            }
            _ => {}
        }
        rest = next;
    }
    false
}

fn slice(full: &[u8], start: usize, max: usize) -> (&[u8], Vec<u8>) {
    let room = max.clamp(1, PDU_MAX);
    let from = start.min(full.len());
    let to = (from + room).min(full.len());
    let mut next = vec![0u8];
    if to < full.len() {
        next = vec![2, (to >> 8) as u8, to as u8];
    }
    (&full[from..to], next)
}

fn continuation(rest: &[u8]) -> usize {
    match rest {
        [2, hi, lo, ..] => (usize::from(*hi) << 8) | usize::from(*lo),
        _ => 0,
    }
}

fn element(body: &[u8]) -> Option<(&[u8], &[u8])> {
    let head = *body.first()?;
    let index = head & 0x07;
    let (len, from): (usize, usize) = match index {
        0 => (1, 1),
        1 => (2, 1),
        2 => (4, 1),
        3 => (8, 1),
        4 => (16, 1),
        5 => (usize::from(*body.get(1)?), 2),
        6 => (usize::from(u16::from_be_bytes([*body.get(1)?, *body.get(2)?])), 3),
        _ => (
            u32::from_be_bytes([*body.get(1)?, *body.get(2)?, *body.get(3)?, *body.get(4)?])
                as usize,
            5,
        ),
    };
    // A nil element carries no payload.
    let len = if head >> 3 == 0 { 0 } else { len };
    let end = from.checked_add(len)?;
    if end > body.len() {
        return None;
    }
    Some((&body[from..end], &body[end..]))
}

fn packet(pdu: u8, tid: u16, params: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + params.len());
    out.push(pdu);
    out.extend_from_slice(&tid.to_be_bytes());
    out.extend_from_slice(&(params.len() as u16).to_be_bytes());
    out.extend_from_slice(params);
    out
}

fn error(tid: u16, code: u16) -> Vec<u8> {
    packet(RSP_ERROR, tid, &code.to_be_bytes())
}

fn uint8(v: u8) -> Vec<u8> {
    vec![0x08, v]
}

fn uint16(v: u16) -> Vec<u8> {
    let mut out = vec![0x09];
    out.extend_from_slice(&v.to_be_bytes());
    out
}

fn uint32(v: u32) -> Vec<u8> {
    let mut out = vec![0x0a];
    out.extend_from_slice(&v.to_be_bytes());
    out
}

fn uuid16(v: u16) -> Vec<u8> {
    let mut out = vec![0x19];
    out.extend_from_slice(&v.to_be_bytes());
    out
}

fn uuid128(v: &[u8; 16]) -> Vec<u8> {
    let mut out = vec![0x1c];
    out.extend_from_slice(v);
    out
}

fn text(v: &str) -> Vec<u8> {
    let mut out = vec![0x25, v.len() as u8];
    out.extend_from_slice(v.as_bytes());
    out
}

fn seq(body: &[u8]) -> Vec<u8> {
    let mut out = match u8::try_from(body.len()) {
        Ok(len) => vec![0x35, len],
        Err(_) => [&[0x36][..], &(body.len() as u16).to_be_bytes()].concat(),
    };
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [u8; 5] = [0x0a, 0x00, 0x00, 0xff, 0xff];

    fn every() -> Vec<&'static Record> {
        Service::ALL.into_iter().flat_map(Service::records).collect()
    }

    #[test]
    fn elements_carry_their_length() {
        assert_eq!(element(&uint8(7)), Some((&[7u8][..], &[][..])));
        let two = [uuid16(0x1002), uint8(3)].concat();
        let (first, rest) = element(&two).expect("first");
        assert_eq!(first, &[0x10, 0x02]);
        assert_eq!(element(rest), Some((&[3u8][..], &[][..])));
    }

    #[test]
    fn only_records_on_offer_are_found() {
        let search = |offered: &[&Record], pattern: &[u8]| {
            let req = packet(REQ_SERVICE_SEARCH, 1, &[seq(pattern), vec![0, 10]].concat());
            answer(&req, offered).windows(4).any(|w| w == CARPLAY.handle.to_be_bytes())
        };
        assert!(search(&every(), &uuid128(&CARPLAY_UUID)));
        assert!(!search(&[&HANDS_FREE_UNIT], &uuid128(&CARPLAY_UUID)));
        assert!(search(&every(), &uuid16(UUID_BROWSE_ROOT)));
    }

    #[test]
    fn a_browse_over_every_record_stays_one_readable_list() {
        let found: Vec<u8> = every().iter().flat_map(|r| record(r, &ALL)).collect();
        assert!(found.len() > 255);
        assert_eq!(element(&seq(&found)), Some((&found[..], &[][..])));
    }

    #[test]
    fn the_hands_free_unit_names_its_profile_channel_and_features() {
        let body = record(&HANDS_FREE_UNIT, &ALL);
        let has = |part: &[u8]| body.windows(part.len()).any(|w| w == part);
        assert!(has(&[uuid16(HANDS_FREE), uuid16(GENERIC_AUDIO)].concat()));
        assert!(has(&[uuid16(UUID_RFCOMM), uint8(7)].concat()));
        assert!(has(&[uint16(0x0311), uint16(HANDS_FREE_FEATURES)].concat()));
    }

    #[test]
    fn our_question_finds_the_channel_in_a_phones_answer() {
        let ask = ask_channel(5, &uuid16(HANDS_FREE_GATEWAY));
        assert_eq!(ask[0], REQ_SEARCH_ATTRIBUTE);
        let gateway = Record {
            handle: 0x0001_0009,
            uuid: full_uuid(HANDS_FREE_GATEWAY),
            channel: 13,
            name: "Gateway",
            profile: Profile::SerialPort,
        };
        let reply = answer(&ask, &[&gateway]);
        assert_eq!(channel_in(&reply), Some(13));
        assert_eq!(channel_in(&error(5, ERROR_SYNTAX)), None);
    }

    #[test]
    fn each_service_names_its_channels() {
        assert_eq!(Service::of_channel(3), Some(Service::CarPlay));
        assert_eq!(Service::of_channel(7), Some(Service::HandsFree));
        assert_eq!(Service::of_channel(8), Some(Service::AndroidAuto));
        assert_eq!(Service::of_channel(9), None);
    }
}
