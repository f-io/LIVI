//! The service records the phone browses for, served on the SDP channel.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_L2CAP: libc::c_int = 0;
const SOCK_SEQPACKET: libc::c_int = 5;
/// The channel every SDP client connects to.
const PSM: u16 = 1;
const PDU_MAX: usize = 1024;

/// Wireless iAP.
pub const IAP_UUID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0xde, 0xca, 0xfa, 0xde, 0xde, 0xca, 0xde, 0xaf, 0xde, 0xca, 0xca, 0xff,
];
/// Wireless iAP the other way round, offered by the phone.
pub const IAP_CLIENT_UUID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0xde, 0xca, 0xfa, 0xde, 0xde, 0xca, 0xde, 0xaf, 0xde, 0xca, 0xca, 0xfe,
];
/// CarPlay.
pub const CARPLAY_UUID: [u8; 16] = [
    0xec, 0x88, 0x43, 0x48, 0xcd, 0x41, 0x40, 0xa2, 0x97, 0x27, 0x57, 0x5d, 0x50, 0xbf, 0x1f, 0xd3,
];

/// One service the phone can open, and where.
pub struct Record {
    pub handle: u32,
    pub uuid: [u8; 16],
    pub channel: u8,
    pub name: &'static str,
}

/// Everything this controller offers.
pub const RECORDS: [Record; 2] = [
    Record {
        handle: 0x0001_0001,
        uuid: IAP_UUID,
        channel: 3,
        name: "Wireless iAP",
    },
    Record {
        handle: 0x0001_0002,
        uuid: CARPLAY_UUID,
        channel: 4,
        name: "CarPlay",
    },
];
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

#[repr(C)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

/// Prints the answer to a plain browse.
pub fn dump() {
    let mut request = Vec::new();
    request.extend_from_slice(&seq(&uuid16(UUID_BROWSE_ROOT)));
    request.extend_from_slice(&u16::MAX.to_be_bytes());
    request.extend_from_slice(&seq(&uint32(0x0000_ffff)));
    request.push(0);
    let reply = answer(&packet(REQ_SEARCH_ATTRIBUTE, 1, &request));
    println!(
        "{}",
        reply.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
}

/// Serves the record until the socket dies, one client at a time.
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

/// Opens the SDP channel.
fn listen() -> Result<OwnedFd, String> {
    let raw = unsafe {
        libc::socket(
            AF_BLUETOOTH,
            SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            BTPROTO_L2CAP,
        )
    };
    if raw < 0 {
        return Err(format!("socket: {}", std::io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrL2 {
        family: AF_BLUETOOTH as libc::sa_family_t,
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

/// One client, request after request.
fn converse(client: OwnedFd) {
    let mut sock = std::fs::File::from(client);
    let mut buf = [0u8; PDU_MAX];
    loop {
        let Ok(n) = sock.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        let reply = answer(&buf[..n]);
        if sock.write_all(&reply).is_err() {
            return;
        }
    }
}

/// Builds the reply to one request, or an error when it cannot be read.
fn answer(req: &[u8]) -> Vec<u8> {
    if req.len() < 5 {
        return error(0, ERROR_SYNTAX);
    }
    let pdu = req[0];
    let tid = u16::from_be_bytes([req[1], req[2]]);
    let body = &req[5..];
    match pdu {
        REQ_SEARCH_ATTRIBUTE => search_attribute(tid, body),
        REQ_ATTRIBUTE => attribute(tid, body),
        REQ_SERVICE_SEARCH => service_search(tid, body),
        _ => error(tid, ERROR_SYNTAX),
    }
}

/// Which records match, and their attributes.
fn search_attribute(tid: u16, body: &[u8]) -> Vec<u8> {
    let Some((pattern, rest)) = element(body) else {
        return error(tid, ERROR_SYNTAX);
    };
    if rest.len() < 2 {
        return error(tid, ERROR_SYNTAX);
    }
    let max = u16::from_be_bytes([rest[0], rest[1]]) as usize;
    let Some((attrs, rest)) = element(&rest[2..]) else {
        return error(tid, ERROR_SYNTAX);
    };
    let start = continuation(rest);
    let mut found = Vec::new();
    for r in RECORDS.iter().filter(|r| wanted(pattern, r)) {
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

/// The same answer, asked for one record by its handle.
fn attribute(tid: u16, body: &[u8]) -> Vec<u8> {
    if body.len() < 6 {
        return error(tid, ERROR_SYNTAX);
    }
    let handle = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let max = u16::from_be_bytes([body[4], body[5]]) as usize;
    let Some((attrs, rest)) = element(&body[6..]) else {
        return error(tid, ERROR_SYNTAX);
    };
    let start = continuation(rest);
    let list = match RECORDS.iter().find(|r| r.handle == handle) {
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

/// Which record handles match.
fn service_search(tid: u16, body: &[u8]) -> Vec<u8> {
    let Some((pattern, _)) = element(body) else {
        return error(tid, ERROR_SYNTAX);
    };
    let hits: Vec<&Record> = RECORDS.iter().filter(|r| wanted(pattern, r)).collect();
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

/// Whether the search pattern asks for this record.
fn wanted(pattern: &[u8], record: &Record) -> bool {
    let mut rest = pattern;
    while let Some((value, next)) = element(rest) {
        let hit = match value.len() {
            2 => {
                let id = u16::from_be_bytes([value[0], value[1]]);
                id == UUID_BROWSE_ROOT || id == UUID_L2CAP || id == UUID_RFCOMM
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

/// One record, keeping only the attributes that were asked for.
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

/// Every attribute of one record, in ascending order.
fn attributes(record: &Record) -> Vec<(u16, Vec<u8>)> {
    let protocol = seq(&[
        seq(&uuid16(UUID_L2CAP)),
        seq(&[uuid16(UUID_RFCOMM), uint8(record.channel)].concat()),
    ]
    .concat());
    let profile = seq(&seq(&[uuid16(UUID_SERIAL_PORT), uint16(0x0100)].concat()));
    vec![
        (0x0000, uint32(record.handle)),
        (0x0001, seq(&uuid128(&record.uuid))),
        (0x0002, uint32(0)),
        (0x0004, protocol),
        (0x0005, seq(&uuid16(UUID_BROWSE_ROOT))),
        (0x0008, uint8(0xff)),
        (0x0009, profile),
        (0x0100, text(record.name)),
    ]
}

/// Whether an attribute id falls inside the requested list of ids and ranges.
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

/// Cuts the answer to what the client takes, and says where to carry on.
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

/// Where a request wants the answer to carry on, from the state it echoed back.
fn continuation(rest: &[u8]) -> usize {
    match rest {
        [2, hi, lo, ..] => ((*hi as usize) << 8) | *lo as usize,
        _ => 0,
    }
}

/// The payload of the next data element, and the rest.
pub(crate) fn element(body: &[u8]) -> Option<(&[u8], &[u8])> {
    let head = *body.first()?;
    let index = head & 0x07;
    let (len, from): (usize, usize) = match index {
        0 => (1, 1),
        1 => (2, 1),
        2 => (4, 1),
        3 => (8, 1),
        4 => (16, 1),
        5 => (*body.get(1)? as usize, 2),
        6 => (
            u16::from_be_bytes([*body.get(1)?, *body.get(2)?]) as usize,
            3,
        ),
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

pub(crate) fn packet(pdu: u8, tid: u16, params: &[u8]) -> Vec<u8> {
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

pub(crate) fn uint16(v: u16) -> Vec<u8> {
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

pub(crate) fn uuid128(v: &[u8; 16]) -> Vec<u8> {
    let mut out = vec![0x1c];
    out.extend_from_slice(v);
    out
}

fn text(v: &str) -> Vec<u8> {
    let mut out = vec![0x25, v.len() as u8];
    out.extend_from_slice(v.as_bytes());
    out
}

pub(crate) fn seq(body: &[u8]) -> Vec<u8> {
    let mut out = vec![0x35, body.len() as u8];
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_carry_their_length() {
        assert_eq!(element(&uint8(7)), Some((&[7u8][..], &[][..])));
        assert_eq!(element(&uuid16(0x1002)), Some((&[0x10, 0x02][..], &[][..])));
        let two = [uuid16(0x1002), uint8(3)].concat();
        let (first, rest) = element(&two).expect("first");
        assert_eq!(first, &[0x10, 0x02]);
        assert_eq!(element(rest), Some((&[3u8][..], &[][..])));
    }

    #[test]
    fn the_browse_root_finds_every_record() {
        for r in &RECORDS {
            assert!(wanted(&uuid16(UUID_BROWSE_ROOT), r));
            assert!(!wanted(&uuid16(0x111e), r));
        }
        assert!(wanted(&uuid128(&CARPLAY_UUID), &RECORDS[1]));
        assert!(!wanted(&uuid128(&CARPLAY_UUID), &RECORDS[0]));
    }

    #[test]
    fn a_range_covers_every_attribute() {
        let all = [0x0a, 0x00, 0x00, 0xff, 0xff];
        assert!(asked(&all, 0x0000));
        assert!(asked(&all, 0x0100));
        let one = uint16(0x0004);
        assert!(asked(&one, 0x0004));
        assert!(!asked(&one, 0x0005));
    }

    #[test]
    fn every_record_names_its_own_channel() {
        let all = [0x0a, 0x00, 0x00, 0xff, 0xff];
        for r in &RECORDS {
            let body = record(r, &all);
            assert!(body.windows(16).any(|w| w == r.uuid));
            assert!(body.contains(&r.channel));
            assert!(String::from_utf8_lossy(&body).contains(r.name));
        }
    }

    #[test]
    fn a_long_answer_is_handed_over_in_pieces() {
        let full: Vec<u8> = (0..100u8).collect();
        let (first, next) = slice(&full, 0, 40);
        assert_eq!(first.len(), 40);
        assert_eq!(next, vec![2, 0, 40]);
        assert_eq!(continuation(&next), 40);
        let (last, done) = slice(&full, 40, 200);
        assert_eq!(last.len(), 60);
        assert_eq!(done, vec![0]);
    }
}
