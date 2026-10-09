//! The HCI packets this host sends and the events it reads, as they cross the tunnel: a packet
//! type byte, then the packet.

pub const COMMAND: u8 = 0x01;
pub const ACL: u8 = 0x02;
pub const SCO: u8 = 0x03;
pub const EVENT: u8 = 0x04;

pub const CREATE_CONNECTION: u16 = 0x0405;
pub const DISCONNECT: u16 = 0x0406;
pub const ACCEPT_CONNECTION: u16 = 0x0409;
pub const LINK_KEY_REPLY: u16 = 0x040b;
pub const LINK_KEY_NEGATIVE_REPLY: u16 = 0x040c;
pub const PIN_CODE_NEGATIVE_REPLY: u16 = 0x040e;
pub const AUTHENTICATION_REQUESTED: u16 = 0x0411;
pub const SET_CONNECTION_ENCRYPTION: u16 = 0x0413;
pub const ACCEPT_SYNC_CONNECTION: u16 = 0x0429;
pub const REJECT_SYNC_CONNECTION: u16 = 0x042a;
pub const IO_CAPABILITY_REPLY: u16 = 0x042b;
pub const USER_CONFIRM_REPLY: u16 = 0x042c;
pub const WRITE_DEFAULT_LINK_POLICY: u16 = 0x080f;
pub const SET_EVENT_MASK: u16 = 0x0c01;
pub const RESET: u16 = 0x0c03;
pub const WRITE_LOCAL_NAME: u16 = 0x0c13;
pub const WRITE_SCAN_ENABLE: u16 = 0x0c1a;
pub const WRITE_CLASS_OF_DEVICE: u16 = 0x0c24;
pub const WRITE_VOICE_SETTING: u16 = 0x0c26;
pub const WRITE_EXTENDED_INQUIRY_RESPONSE: u16 = 0x0c52;
pub const WRITE_SIMPLE_PAIRING_MODE: u16 = 0x0c56;
pub const READ_BUFFER_SIZE: u16 = 0x1005;
pub const READ_BD_ADDR: u16 = 0x1009;

/// ACL packets start a PDU with this, and carry one on with CONTINUING.
pub const START: u8 = 0x02;
pub const CONTINUING: u8 = 0x01;

/// The address as it travels, least significant byte first.
pub type Addr = [u8; 6];

/// The usual notation, most significant byte first.
pub fn text(addr: &Addr) -> String {
    addr.iter().rev().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

pub fn parse_addr(text: &str) -> Option<Addr> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in out.iter_mut().rev() {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

pub fn command(opcode: u16, params: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + params.len());
    out.push(COMMAND);
    out.extend_from_slice(&opcode.to_le_bytes());
    out.push(params.len() as u8);
    out.extend_from_slice(params);
    out
}

pub fn acl(handle: u16, boundary: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + data.len());
    out.push(ACL);
    out.extend_from_slice(&(handle | (u16::from(boundary) << 12)).to_le_bytes());
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub fn sco(handle: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.push(SCO);
    out.extend_from_slice(&handle.to_le_bytes());
    out.push(data.len() as u8);
    out.extend_from_slice(data);
    out
}

/// An ACL packet's handle, whether it starts a PDU, and its data.
pub fn read_acl(pkt: &[u8]) -> Option<(u16, bool, &[u8])> {
    let head = u16::from_le_bytes([*pkt.get(1)?, *pkt.get(2)?]);
    let len = usize::from(u16::from_le_bytes([*pkt.get(3)?, *pkt.get(4)?]));
    let data = pkt.get(5..5 + len)?;
    let boundary = ((head >> 12) & 0x3) as u8;
    Some((head & 0x0fff, boundary != CONTINUING, data))
}

pub fn read_sco(pkt: &[u8]) -> Option<(u16, &[u8])> {
    let head = u16::from_le_bytes([*pkt.get(1)?, *pkt.get(2)?]);
    let len = usize::from(*pkt.get(3)?);
    Some((head & 0x0fff, pkt.get(4..4 + len)?))
}

#[derive(Debug, PartialEq)]
pub enum Event {
    CommandComplete { opcode: u16, credits: u8, ret: Vec<u8> },
    CommandStatus { opcode: u16, credits: u8, status: u8 },
    ConnectionRequest { addr: Addr, link: u8 },
    ConnectionComplete { status: u8, handle: u16, addr: Addr, link: u8 },
    DisconnectionComplete { status: u8, handle: u16, reason: u8 },
    AuthenticationComplete { status: u8, handle: u16 },
    EncryptionChange { status: u8, handle: u16, on: bool },
    HardwareError(u8),
    NumberOfCompletedPackets(Vec<(u16, u16)>),
    PinCodeRequest(Addr),
    LinkKeyRequest(Addr),
    LinkKeyNotification { addr: Addr, key: [u8; 16], kind: u8 },
    DataBufferOverflow(u8),
    SyncConnectionComplete(Sync),
    IoCapabilityRequest(Addr),
    IoCapabilityResponse { addr: Addr, io: u8, auth: u8 },
    UserConfirmationRequest(Addr),
    SimplePairingComplete { status: u8, addr: Addr },
    Other(u8),
}

#[derive(Debug, PartialEq)]
pub struct Sync {
    pub status: u8,
    pub handle: u16,
    pub addr: Addr,
    pub link: u8,
    /// In slots of 625 µs.
    pub interval: u8,
    pub rx_len: u16,
    pub tx_len: u16,
    pub air_mode: u8,
}

pub const LINK_SCO: u8 = 0x00;
pub const LINK_ACL: u8 = 0x01;
pub const LINK_ESCO: u8 = 0x02;

fn addr_at(p: &[u8], at: usize) -> Option<Addr> {
    p.get(at..at + 6)?.try_into().ok()
}

fn u16_at(p: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*p.get(at)?, *p.get(at + 1)?]))
}

/// A whole event packet, type byte included.
pub fn event(pkt: &[u8]) -> Option<Event> {
    if pkt.first() != Some(&EVENT) {
        return None;
    }
    let code = *pkt.get(1)?;
    let len = usize::from(*pkt.get(2)?);
    let p = pkt.get(3..3 + len)?;
    Some(match code {
        0x03 => Event::ConnectionComplete {
            status: *p.first()?,
            handle: u16_at(p, 1)? & 0x0fff,
            addr: addr_at(p, 3)?,
            link: *p.get(9)?,
        },
        0x04 => Event::ConnectionRequest { addr: addr_at(p, 0)?, link: *p.get(9)? },
        0x05 => Event::DisconnectionComplete {
            status: *p.first()?,
            handle: u16_at(p, 1)? & 0x0fff,
            reason: *p.get(3)?,
        },
        0x06 => {
            Event::AuthenticationComplete { status: *p.first()?, handle: u16_at(p, 1)? & 0x0fff }
        }
        0x08 => Event::EncryptionChange {
            status: *p.first()?,
            handle: u16_at(p, 1)? & 0x0fff,
            on: *p.get(3)? != 0,
        },
        0x0e => Event::CommandComplete {
            credits: *p.first()?,
            opcode: u16_at(p, 1)?,
            ret: p.get(3..)?.to_vec(),
        },
        0x0f => {
            Event::CommandStatus { status: *p.first()?, credits: *p.get(1)?, opcode: u16_at(p, 2)? }
        }
        0x10 => Event::HardwareError(*p.first()?),
        0x13 => {
            let count = usize::from(*p.first()?);
            let mut done = Vec::with_capacity(count);
            for i in 0..count {
                done.push((u16_at(p, 1 + i * 4)? & 0x0fff, u16_at(p, 3 + i * 4)?));
            }
            Event::NumberOfCompletedPackets(done)
        }
        0x16 => Event::PinCodeRequest(addr_at(p, 0)?),
        0x17 => Event::LinkKeyRequest(addr_at(p, 0)?),
        0x18 => Event::LinkKeyNotification {
            addr: addr_at(p, 0)?,
            key: p.get(6..22)?.try_into().ok()?,
            kind: *p.get(22)?,
        },
        0x1a => Event::DataBufferOverflow(*p.first()?),
        0x2c => Event::SyncConnectionComplete(Sync {
            status: *p.first()?,
            handle: u16_at(p, 1)? & 0x0fff,
            addr: addr_at(p, 3)?,
            link: *p.get(9)?,
            interval: *p.get(10)?,
            rx_len: u16_at(p, 12)?,
            tx_len: u16_at(p, 14)?,
            air_mode: *p.get(16)?,
        }),
        0x31 => Event::IoCapabilityRequest(addr_at(p, 0)?),
        0x32 => {
            Event::IoCapabilityResponse { addr: addr_at(p, 0)?, io: *p.get(6)?, auth: *p.get(8)? }
        }
        0x33 => Event::UserConfirmationRequest(addr_at(p, 0)?),
        0x36 => Event::SimplePairingComplete { status: *p.first()?, addr: addr_at(p, 1)? },
        other => Event::Other(other),
    })
}

/// The controller's words for the failures a phone or a pairing runs into.
pub fn reason(status: u8) -> &'static str {
    match status {
        0x00 => "success",
        0x02 => "unknown connection",
        0x04 => "page timeout",
        0x05 => "authentication failure",
        0x06 => "no key",
        0x08 => "connection timeout",
        0x0b => "connection already exists",
        0x0c => "command disallowed",
        0x0d => "rejected, no resources",
        0x0e => "rejected for security",
        0x0f => "rejected, unacceptable address",
        0x10 => "accept timeout",
        0x11 => "unsupported feature or parameter",
        0x12 => "invalid parameters",
        0x13 => "the phone ended it",
        0x16 => "we ended it",
        0x18 => "pairing not allowed",
        0x1a => "unsupported remote feature",
        0x1e => "invalid parameters on the air",
        0x1f => "unspecified error",
        0x22 => "response timeout on the air",
        0x28 => "instant passed",
        0x2f => "insufficient security",
        _ => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_reads_back_the_way_it_is_written() {
        let addr = parse_addr("94:45:60:A2:1B:BA").unwrap();
        assert_eq!(addr, [0xba, 0x1b, 0xa2, 0x60, 0x45, 0x94]);
        assert_eq!(text(&addr), "94:45:60:A2:1B:BA");
        assert_eq!(parse_addr("94:45:60"), None);
    }

    #[test]
    fn commands_and_data_carry_their_headers() {
        assert_eq!(command(RESET, &[]), [0x01, 0x03, 0x0c, 0x00]);
        assert_eq!(acl(0x002a, START, &[9, 9]), [0x02, 0x2a, 0x20, 0x02, 0x00, 9, 9]);
        assert_eq!(read_acl(&acl(0x002a, CONTINUING, &[7])), Some((0x002a, false, &[7u8][..])));
        assert_eq!(sco(0x0101, &[1, 2]), [0x03, 0x01, 0x01, 0x02, 1, 2]);
        assert_eq!(read_sco(&[0x03, 0x01, 0x31, 0x01, 5]), Some((0x0101, &[5u8][..])));
    }

    #[test]
    fn events_name_their_fields() {
        let complete = [0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
        assert_eq!(
            event(&complete),
            Some(Event::CommandComplete { opcode: RESET, credits: 1, ret: vec![0] })
        );
        let done = [0x04, 0x13, 0x09, 0x02, 0x2a, 0x00, 0x03, 0x00, 0x2b, 0x00, 0x01, 0x00];
        assert_eq!(event(&done), Some(Event::NumberOfCompletedPackets(vec![(42, 3), (43, 1)])));
        let mut sync = vec![0x04, 0x2c, 0x11, 0x00, 0x01, 0x01];
        sync.extend_from_slice(&[1, 2, 3, 4, 5, 6, LINK_ESCO, 12, 4, 60, 0, 60, 0, 2]);
        let Some(Event::SyncConnectionComplete(s)) = event(&sync) else { panic!("{sync:?}") };
        assert_eq!((s.handle, s.link, s.interval, s.rx_len, s.air_mode), (0x0101, 2, 12, 60, 2));
        assert_eq!(event(&[0x04, 0x13, 0x05, 0x02, 0x2a]), None);
    }
}
