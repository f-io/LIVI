//! RFCOMM over one L2CAP channel: the multiplexer on DLCI 0 and one channel per DLCI, paced by
//! credits.

use std::collections::BTreeMap;

const SABM: u8 = 0x2f;
const UA: u8 = 0x63;
const DM: u8 = 0x0f;
const DISC: u8 = 0x43;
const UIH: u8 = 0xef;
const PF: u8 = 0x10;

const PN: u8 = 0x20;
const MSC: u8 = 0x38;
const RPN: u8 = 0x24;
const RLS: u8 = 0x14;
const TEST: u8 = 0x08;
const FCON: u8 = 0x28;
const FCOFF: u8 = 0x18;
const NSC: u8 = 0x04;

/// Credit based flow control asked for, and granted.
const CFC_ASK: u8 = 0xf0;
const CFC_GRANT: u8 = 0xe0;
/// What a peer gets to send before it hears from us again.
const CREDITS: u8 = 7;
/// The largest frame we take, which fits our L2CAP MTU with its header.
pub const FRAME: usize = 1000;
/// Without a parameter negotiation the frame size stays small.
const DEFAULT_FRAME: usize = 127;
/// RTC, RTR and DV up.
const SIGNALS: u8 = 0x8d;
/// 9600 8N1 without flow control, the answer to a port query.
const PORT_DEFAULTS: [u8; 7] = [0x03, 0x03, 0x00, 0x11, 0x13, 0xff, 0x3f];

const fn crc_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u8;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xe0 } else { c >> 1 };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC: [u8; 256] = crc_table();

fn fcs(bytes: &[u8]) -> u8 {
    0xff - bytes.iter().fold(0xffu8, |crc, b| CRC[usize::from(crc ^ b)])
}

fn address(dlci: u8, cr: bool) -> u8 {
    (dlci << 2) | (u8::from(cr) << 1) | 1
}

fn frame(addr: u8, control: u8, credits: Option<u8>, info: &[u8]) -> Vec<u8> {
    let mut out = vec![addr, control];
    if info.len() <= 127 {
        out.push(((info.len() as u8) << 1) | 1);
    } else {
        out.push(((info.len() & 0x7f) as u8) << 1);
        out.push((info.len() >> 7) as u8);
    }
    out.extend(credits);
    out.extend_from_slice(info);
    // Data frames check only address and control, the others their length as well.
    let covered = if control & !PF == UIH { 2 } else { 3 };
    out.push(fcs(&out[..covered]));
    out
}

fn mcc(kind: u8, command: bool, values: &[u8]) -> Vec<u8> {
    let mut out = vec![(kind << 2) | (u8::from(command) << 1) | 1, ((values.len() as u8) << 1) | 1];
    out.extend_from_slice(values);
    out
}

struct Frame<'a> {
    dlci: u8,
    control: u8,
    pf: bool,
    info: &'a [u8],
}

fn parse(bytes: &[u8]) -> Option<Frame<'_>> {
    let addr = *bytes.first()?;
    let control = *bytes.get(1)?;
    let first = *bytes.get(2)?;
    let (len, at) = if first & 1 == 1 {
        (usize::from(first >> 1), 3)
    } else {
        (usize::from(first >> 1) | (usize::from(*bytes.get(3)?) << 7), 4)
    };
    let pf = control & PF != 0;
    // A data frame with the poll bit carries a credit byte in front of its data.
    let credit = usize::from(control & !PF == UIH && pf && addr >> 2 != 0);
    let info = bytes.get(at..at + credit + len)?;
    let covered = if control & !PF == UIH { 2 } else { at };
    if *bytes.get(at + credit + len)? != fcs(&bytes[..covered]) {
        return None;
    }
    Some(Frame { dlci: addr >> 2, control: control & !PF, pf, info })
}

#[derive(Debug)]
struct Dlc {
    open: bool,
    frame: usize,
    cfc: bool,
    /// Frames we may still send.
    may_send: u32,
    /// Frames the peer sent that we have not paid back yet.
    owed: u8,
}

/// What came of a frame, each channel named by its DLCI.
#[derive(Debug, PartialEq)]
pub enum Happened {
    MuxOpen,
    MuxClosed,
    Opened(u8),
    Refused(u8),
    Closed(u8),
    Data(u8, Vec<u8>),
    /// Credits came in, so a waiting sender may go on.
    CanSend(u8),
}

pub struct Session {
    initiator: bool,
    open: bool,
    /// The largest frame the L2CAP channel carries to the peer.
    room: usize,
    dlcs: BTreeMap<u8, Dlc>,
}

impl Session {
    pub fn new(initiator: bool, peer_mtu: u16) -> Self {
        // Address, control, two length bytes, a credit and the check sum.
        let room = usize::from(peer_mtu).saturating_sub(6).max(DEFAULT_FRAME);
        Self { initiator, open: false, room, dlcs: BTreeMap::new() }
    }

    pub fn initiator(&self) -> bool {
        self.initiator
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The DLCI of a server channel, which says whether the channel sits on the side that started
    /// the session.
    pub fn dlci(&self, channel: u8, theirs: bool) -> u8 {
        (channel << 1) | u8::from(theirs != self.initiator)
    }

    pub fn channel_open(&self, dlci: u8) -> bool {
        self.dlcs.get(&dlci).is_some_and(|d| d.open)
    }

    pub fn frame_size(&self, dlci: u8) -> usize {
        self.dlcs.get(&dlci).map_or(DEFAULT_FRAME, |d| d.frame)
    }

    pub fn start(&mut self) -> Vec<u8> {
        frame(address(0, self.initiator), SABM | PF, None, &[])
    }

    /// Opens a channel on the peer, with the parameter negotiation first.
    pub fn open(&mut self, channel: u8) -> Vec<u8> {
        let dlci = self.dlci(channel, true);
        let size = FRAME.min(self.room);
        self.dlcs.insert(dlci, Dlc { open: false, frame: size, cfc: false, may_send: 0, owed: 0 });
        let mut values = vec![dlci, CFC_ASK, 0, 0];
        values.extend_from_slice(&(size as u16).to_le_bytes());
        values.extend_from_slice(&[0, CREDITS]);
        self.uih(0, &mcc(PN, true, &values))
    }

    pub fn close(&mut self, dlci: u8) -> Option<Vec<u8>> {
        self.dlcs.remove(&dlci)?;
        Some(frame(address(dlci, self.initiator), DISC | PF, None, &[]))
    }

    fn uih(&self, dlci: u8, info: &[u8]) -> Vec<u8> {
        frame(address(dlci, self.initiator), UIH, None, info)
    }

    fn reply(&self, dlci: u8, control: u8) -> Vec<u8> {
        frame(address(dlci, !self.initiator), control | PF, None, &[])
    }

    /// One frame of data if a credit allows it, with the credits we owe riding along.
    pub fn send(&mut self, dlci: u8, data: &[u8]) -> Option<Vec<u8>> {
        let initiator = self.initiator;
        let dlc = self.dlcs.get_mut(&dlci).filter(|d| d.open)?;
        if dlc.cfc && dlc.may_send == 0 {
            return None;
        }
        let addr = address(dlci, initiator);
        if !dlc.cfc {
            return Some(frame(addr, UIH, None, data));
        }
        dlc.may_send -= 1;
        let owed = std::mem::take(&mut dlc.owed);
        Some(if owed > 0 {
            frame(addr, UIH | PF, Some(owed), data)
        } else {
            frame(addr, UIH, None, data)
        })
    }

    /// After a frame has been handed on: the credits back once half of them are spent.
    pub fn consumed(&mut self, dlci: u8) -> Option<Vec<u8>> {
        let initiator = self.initiator;
        let dlc = self.dlcs.get_mut(&dlci).filter(|d| d.open && d.cfc)?;
        if dlc.owed < CREDITS / 2 + 1 {
            return None;
        }
        let owed = std::mem::take(&mut dlc.owed);
        Some(frame(address(dlci, initiator), UIH | PF, Some(owed), &[]))
    }

    pub fn input(
        &mut self,
        bytes: &[u8],
        listening: impl Fn(u8) -> bool,
    ) -> (Vec<Vec<u8>>, Vec<Happened>) {
        let (mut out, mut happened) = (Vec::new(), Vec::new());
        let Some(f) = parse(bytes) else {
            return (out, happened);
        };
        let channel = f.dlci >> 1;
        match (f.control, f.dlci) {
            (SABM, 0) => {
                self.open = true;
                out.push(self.reply(0, UA));
                happened.push(Happened::MuxOpen);
            }
            (SABM, dlci) => {
                if self.open && listening(channel) {
                    let dlc = self.dlcs.entry(dlci).or_insert(Dlc {
                        open: false,
                        frame: DEFAULT_FRAME,
                        cfc: false,
                        may_send: 0,
                        owed: 0,
                    });
                    dlc.open = true;
                    out.push(self.reply(dlci, UA));
                    out.push(self.uih(0, &mcc(MSC, true, &[(dlci << 2) | 0x03, SIGNALS])));
                    happened.push(Happened::Opened(dlci));
                } else {
                    self.dlcs.remove(&dlci);
                    out.push(self.reply(dlci, DM));
                }
            }
            (UA, 0) => {
                if !self.open {
                    self.open = true;
                    happened.push(Happened::MuxOpen);
                }
            }
            (UA, dlci) => {
                if let Some(dlc) = self.dlcs.get_mut(&dlci)
                    && !dlc.open
                {
                    dlc.open = true;
                    out.push(self.uih(0, &mcc(MSC, true, &[(dlci << 2) | 0x03, SIGNALS])));
                    happened.push(Happened::Opened(dlci));
                }
            }
            (DM, 0) => {
                self.open = false;
                happened.push(Happened::MuxClosed);
            }
            (DM, dlci) => {
                if let Some(dlc) = self.dlcs.remove(&dlci) {
                    happened.push(if dlc.open {
                        Happened::Closed(dlci)
                    } else {
                        Happened::Refused(dlci)
                    });
                }
            }
            (DISC, 0) => {
                out.push(self.reply(0, UA));
                self.open = false;
                for (dlci, dlc) in std::mem::take(&mut self.dlcs) {
                    if dlc.open {
                        happened.push(Happened::Closed(dlci));
                    }
                }
                happened.push(Happened::MuxClosed);
            }
            (DISC, dlci) => {
                out.push(self.reply(dlci, UA));
                if self.dlcs.remove(&dlci).is_some_and(|d| d.open) {
                    happened.push(Happened::Closed(dlci));
                }
            }
            (UIH, 0) => out.extend(self.control(f.info)),
            (UIH, dlci) => {
                let Some(dlc) = self.dlcs.get_mut(&dlci).filter(|d| d.open) else {
                    return (out, happened);
                };
                let mut data = f.info;
                if f.pf
                    && let Some((credits, rest)) = data.split_first()
                {
                    if dlc.cfc {
                        dlc.may_send += u32::from(*credits);
                        happened.push(Happened::CanSend(dlci));
                    }
                    data = rest;
                }
                if !data.is_empty() {
                    if dlc.cfc {
                        dlc.owed = dlc.owed.saturating_add(1);
                    }
                    happened.push(Happened::Data(dlci, data.to_vec()));
                }
            }
            _ => {}
        }
        (out, happened)
    }

    /// The multiplexer's own commands on DLCI 0.
    fn control(&mut self, info: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut rest = info;
        while rest.len() >= 2 {
            let (kind, command) = (rest[0] >> 2, rest[0] & 0x02 != 0);
            let len = usize::from(rest[1] >> 1);
            let Some(values) = rest.get(2..2 + len) else { break };
            rest = &rest[2 + len..];
            match (kind, command) {
                (PN, true) if values.len() >= 8 => {
                    let dlci = values[0] & 0x3f;
                    let asked = usize::from(u16::from_le_bytes([values[4], values[5]]));
                    let size = asked.clamp(1, FRAME.min(self.room));
                    let cfc = values[1] & 0xf0 == CFC_ASK;
                    let dlc = self.dlcs.entry(dlci).or_insert(Dlc {
                        open: false,
                        frame: size,
                        cfc,
                        may_send: 0,
                        owed: 0,
                    });
                    dlc.frame = size;
                    dlc.cfc = cfc;
                    dlc.may_send = if cfc { u32::from(values[7] & 0x07) } else { 0 };
                    let mut reply = vec![dlci, if cfc { CFC_GRANT } else { 0 }, values[2], 0];
                    reply.extend_from_slice(&(size as u16).to_le_bytes());
                    reply.extend_from_slice(&[0, if cfc { CREDITS } else { 0 }]);
                    out.push(self.uih(0, &mcc(PN, false, &reply)));
                }
                (PN, false) if values.len() >= 8 => {
                    let dlci = values[0] & 0x3f;
                    let initiator = self.initiator;
                    if let Some(dlc) = self.dlcs.get_mut(&dlci) {
                        let size = usize::from(u16::from_le_bytes([values[4], values[5]]));
                        dlc.frame = size.clamp(1, dlc.frame);
                        dlc.cfc = values[1] & 0xf0 == CFC_GRANT;
                        dlc.may_send = if dlc.cfc { u32::from(values[7] & 0x07) } else { 0 };
                        out.push(frame(address(dlci, initiator), SABM | PF, None, &[]));
                    }
                }
                (MSC, true) => out.push(self.uih(0, &mcc(MSC, false, values))),
                (RPN, true) => {
                    let mut reply = values.to_vec();
                    if reply.len() == 1 {
                        reply.extend_from_slice(&PORT_DEFAULTS);
                    }
                    out.push(self.uih(0, &mcc(RPN, false, &reply)));
                }
                (RLS | TEST, true) => out.push(self.uih(0, &mcc(kind, false, values))),
                (FCON | FCOFF, true) => out.push(self.uih(0, &mcc(kind, false, &[]))),
                (_, false) => {}
                (_, true) => out.push(self.uih(0, &mcc(NSC, false, &[(kind << 2) | 0x03]))),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_sum_matches_a_known_frame() {
        // SABM on DLCI 0 from the initiator.
        assert_eq!(frame(0x03, SABM | PF, None, &[]), [0x03, 0x3f, 0x01, 0x1c]);
        // Its answer.
        assert_eq!(frame(0x03, UA | PF, None, &[]), [0x03, 0x73, 0x01, 0xd7]);
    }

    fn phone() -> Session {
        // The phone starts the session, so it is the initiator and we answer.
        Session::new(true, 1013)
    }

    fn us() -> Session {
        Session::new(false, 1013)
    }

    fn talk(from: &mut Session, to: &mut Session, frames: Vec<Vec<u8>>) -> Vec<Happened> {
        let mut happened = Vec::new();
        let mut queue = frames;
        let mut turn = true;
        while !queue.is_empty() {
            let mut next = Vec::new();
            for f in queue {
                let (out, h) =
                    if turn { to.input(&f, |c| c == 7) } else { from.input(&f, |_| false) };
                next.extend(out);
                happened.extend(h);
            }
            queue = next;
            turn = !turn;
        }
        happened
    }

    #[test]
    fn a_phone_opens_our_channel_and_data_flows_on_credits() {
        let (mut phone, mut us) = (phone(), us());
        let start = phone.start();
        let h = talk(&mut phone, &mut us, vec![start]);
        assert!(h.contains(&Happened::MuxOpen));
        assert!(us.is_open() && phone.is_open());
        let pn = phone.open(7);
        let h = talk(&mut phone, &mut us, vec![pn]);
        assert!(h.contains(&Happened::Opened(14)));
        let dlci = phone.dlci(7, true);
        assert_eq!(dlci, 14);
        assert!(us.channel_open(dlci) && phone.channel_open(dlci));
        assert_eq!(us.frame_size(dlci), FRAME);

        for _ in 0..CREDITS {
            let data = phone.send(dlci, b"AT+BRSF=1\r").expect("credit");
            let (_, h) = us.input(&data, |_| true);
            assert_eq!(h, vec![Happened::Data(14, b"AT+BRSF=1\r".to_vec())]);
        }
        assert_eq!(phone.send(dlci, b"x"), None);
        let back = us.consumed(dlci).expect("credits back");
        let (_, h) = phone.input(&back, |_| false);
        assert!(h.contains(&Happened::CanSend(14)));
        assert!(phone.send(dlci, b"x").is_some());
    }

    #[test]
    fn a_channel_nobody_listens_on_is_turned_down() {
        let (mut phone, mut us) = (phone(), us());
        let start = phone.start();
        talk(&mut phone, &mut us, vec![start]);
        let pn = phone.open(9);
        let h = talk(&mut phone, &mut us, vec![pn]);
        assert!(h.contains(&Happened::Refused(18)));
    }

    #[test]
    fn our_own_channel_on_the_phone_opens_and_closes() {
        let mut phone = Session::new(false, 1013);
        let mut ours = Session::new(true, 1013);
        let start = ours.start();
        let (out, h) = phone.input(&start, |_| false);
        assert_eq!(h, vec![Happened::MuxOpen]);
        let (_, h) = ours.input(&out[0], |_| false);
        assert_eq!(h, vec![Happened::MuxOpen]);
        let pn = ours.open(7);
        let (out, _) = phone.input(&pn, |c| c == 7);
        let (sabm, _) = ours.input(&out[0], |_| false);
        let (ua, h) = phone.input(&sabm[0], |c| c == 7);
        assert_eq!(h, vec![Happened::Opened(14)]);
        let (_, h) = ours.input(&ua[0], |_| false);
        assert_eq!(h, vec![Happened::Opened(14)]);
        let bye = ours.close(14).unwrap();
        let (_, h) = phone.input(&bye, |_| false);
        assert_eq!(h, vec![Happened::Closed(14)]);
    }

    #[test]
    fn long_frames_take_two_length_bytes() {
        let data = vec![0x55u8; 300];
        let f = frame(address(14, true), UIH, None, &data);
        let parsed = parse(&f).unwrap();
        assert_eq!(parsed.info, &data[..]);
        let mut broken = f.clone();
        *broken.last_mut().unwrap() ^= 1;
        assert!(parse(&broken).is_none());
    }
}
