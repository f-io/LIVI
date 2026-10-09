//! L2CAP in basic mode: PDUs cut to and joined from ACL packets, and the signalling channel that
//! opens and closes the channels on one link.

use crate::hci;

pub const SIGNALLING: u16 = 0x0001;
pub const PSM_SDP: u16 = 0x0001;
pub const PSM_RFCOMM: u16 = 0x0003;
/// The largest PDU we take, room for an RFCOMM frame of 1000 bytes.
pub const OUR_MTU: u16 = 1013;
/// What a peer that names no MTU takes.
const DEFAULT_MTU: u16 = 672;
const FIRST_DYNAMIC: u16 = 0x0040;

const COMMAND_REJECT: u8 = 0x01;
const CONNECTION_REQUEST: u8 = 0x02;
const CONNECTION_RESPONSE: u8 = 0x03;
const CONFIGURE_REQUEST: u8 = 0x04;
const CONFIGURE_RESPONSE: u8 = 0x05;
const DISCONNECTION_REQUEST: u8 = 0x06;
const DISCONNECTION_RESPONSE: u8 = 0x07;
const ECHO_REQUEST: u8 = 0x08;
const ECHO_RESPONSE: u8 = 0x09;
const INFORMATION_REQUEST: u8 = 0x0a;
const INFORMATION_RESPONSE: u8 = 0x0b;

const OPTION_MTU: u8 = 0x01;
const OPTION_HINT: u8 = 0x80;
const RESULT_SUCCESS: u16 = 0x0000;
const RESULT_PENDING: u16 = 0x0001;
const RESULT_NO_PSM: u16 = 0x0002;
const CONFIG_UNKNOWN_OPTIONS: u16 = 0x0003;
const INFO_EXTENDED_FEATURES: u16 = 0x0002;
const INFO_NOT_SUPPORTED: u16 = 0x0001;

pub fn pdu(cid: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(&cid.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// The ACL packets one PDU goes out in.
pub fn split(handle: u16, pdu: &[u8], acl_mtu: usize) -> Vec<Vec<u8>> {
    pdu.chunks(acl_mtu.max(1))
        .enumerate()
        .map(|(i, part)| hci::acl(handle, if i == 0 { hci::START } else { hci::CONTINUING }, part))
        .collect()
}

/// Joins the ACL packets of one link into whole PDUs.
#[derive(Default)]
pub struct Joiner {
    partial: Vec<u8>,
}

impl Joiner {
    /// The channel and payload once a PDU is complete.
    pub fn push(&mut self, start: bool, data: &[u8]) -> Option<(u16, Vec<u8>)> {
        if start {
            self.partial.clear();
        } else if self.partial.is_empty() {
            return None;
        }
        self.partial.extend_from_slice(data);
        if self.partial.len() < 4 {
            return None;
        }
        let len = usize::from(u16::from_le_bytes([self.partial[0], self.partial[1]]));
        if self.partial.len() < 4 + len {
            return None;
        }
        let cid = u16::from_le_bytes([self.partial[2], self.partial[3]]);
        let payload = self.partial[4..4 + len].to_vec();
        self.partial.clear();
        Some((cid, payload))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// Our connection request is out.
    Asked,
    Configuring {
        ours: bool,
        theirs: bool,
    },
    Open,
}

#[derive(Debug)]
pub struct Channel {
    pub psm: u16,
    pub local: u16,
    pub remote: u16,
    /// The largest PDU the peer takes.
    pub peer_mtu: u16,
    pub ours: bool,
    state: State,
}

impl Channel {
    pub fn open(&self) -> bool {
        self.state == State::Open
    }
}

#[derive(Debug, PartialEq)]
pub enum Signal {
    Opened { local: u16, psm: u16, ours: bool },
    Refused { local: u16 },
    Closed { local: u16 },
}

/// The channels of one ACL link.
pub struct Link {
    pub channels: Vec<Channel>,
    next_cid: u16,
    ident: u8,
}

impl Default for Link {
    fn default() -> Self {
        Self { channels: Vec::new(), next_cid: FIRST_DYNAMIC, ident: 0 }
    }
}

fn command(code: u8, ident: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![code, ident];
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    pdu(SIGNALLING, &out)
}

fn le(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl Link {
    pub fn channel(&self, local: u16) -> Option<&Channel> {
        self.channels.iter().find(|c| c.local == local)
    }

    fn channel_mut(&mut self, local: u16) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.local == local)
    }

    fn next_ident(&mut self) -> u8 {
        self.ident = self.ident.wrapping_add(1).max(1);
        self.ident
    }

    fn allocate(&mut self) -> u16 {
        while self.channels.iter().any(|c| c.local == self.next_cid) {
            self.next_cid = self.next_cid.wrapping_add(1).max(FIRST_DYNAMIC);
        }
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1).max(FIRST_DYNAMIC);
        cid
    }

    pub fn connect(&mut self, psm: u16) -> (u16, Vec<u8>) {
        let local = self.allocate();
        self.channels.push(Channel {
            psm,
            local,
            remote: 0,
            peer_mtu: DEFAULT_MTU,
            ours: true,
            state: State::Asked,
        });
        let ident = self.next_ident();
        (local, command(CONNECTION_REQUEST, ident, &le(&[psm, local])))
    }

    pub fn disconnect(&mut self, local: u16) -> Option<Vec<u8>> {
        let remote = self.channel(local)?.remote;
        self.channels.retain(|c| c.local != local);
        let ident = self.next_ident();
        Some(command(DISCONNECTION_REQUEST, ident, &le(&[remote, local])))
    }

    fn configure(&mut self, local: u16) -> Option<Vec<u8>> {
        let remote = self.channel(local)?.remote;
        let ident = self.next_ident();
        let mut data = le(&[remote, 0]);
        data.extend_from_slice(&[OPTION_MTU, 2]);
        data.extend_from_slice(&OUR_MTU.to_le_bytes());
        Some(command(CONFIGURE_REQUEST, ident, &data))
    }

    /// Marks one side of the configuration done, and says when that opened the channel.
    fn configured(&mut self, local: u16, by_us: bool) -> Option<Signal> {
        let channel = self.channel_mut(local)?;
        let State::Configuring { mut ours, mut theirs } = channel.state else {
            return None;
        };
        if by_us {
            ours = true;
        } else {
            theirs = true;
        }
        channel.state =
            if ours && theirs { State::Open } else { State::Configuring { ours, theirs } };
        (channel.state == State::Open).then_some(Signal::Opened {
            local,
            psm: channel.psm,
            ours: channel.ours,
        })
    }

    /// One PDU on the signalling channel, which may carry several commands.
    pub fn signal(
        &mut self,
        mut payload: &[u8],
        serves: impl Fn(u16) -> bool,
    ) -> (Vec<Vec<u8>>, Vec<Signal>) {
        let (mut out, mut signals) = (Vec::new(), Vec::new());
        while payload.len() >= 4 {
            let (code, ident) = (payload[0], payload[1]);
            let len = usize::from(u16::from_le_bytes([payload[2], payload[3]]));
            let Some(data) = payload.get(4..4 + len) else { break };
            self.one(code, ident, data, &serves, &mut out, &mut signals);
            payload = &payload[4 + len..];
        }
        (out, signals)
    }

    fn one(
        &mut self,
        code: u8,
        ident: u8,
        data: &[u8],
        serves: &impl Fn(u16) -> bool,
        out: &mut Vec<Vec<u8>>,
        signals: &mut Vec<Signal>,
    ) {
        let word = |at: usize| data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        match code {
            CONNECTION_REQUEST => {
                let (Some(psm), Some(remote)) = (word(0), word(2)) else { return };
                if !serves(psm) {
                    out.push(command(
                        CONNECTION_RESPONSE,
                        ident,
                        &le(&[0, remote, RESULT_NO_PSM, 0]),
                    ));
                    return;
                }
                let local = self.allocate();
                self.channels.push(Channel {
                    psm,
                    local,
                    remote,
                    peer_mtu: DEFAULT_MTU,
                    ours: false,
                    state: State::Configuring { ours: false, theirs: false },
                });
                out.push(command(
                    CONNECTION_RESPONSE,
                    ident,
                    &le(&[local, remote, RESULT_SUCCESS, 0]),
                ));
                out.extend(self.configure(local));
            }
            CONNECTION_RESPONSE => {
                let (Some(remote), Some(local), Some(result)) = (word(0), word(2), word(4)) else {
                    return;
                };
                match result {
                    RESULT_PENDING => {}
                    RESULT_SUCCESS => {
                        if let Some(c) = self.channel_mut(local)
                            && c.state == State::Asked
                        {
                            c.remote = remote;
                            c.state = State::Configuring { ours: false, theirs: false };
                            out.extend(self.configure(local));
                        }
                    }
                    _ => {
                        if self.channel(local).is_some() {
                            self.channels.retain(|c| c.local != local);
                            signals.push(Signal::Refused { local });
                        }
                    }
                }
            }
            CONFIGURE_REQUEST => {
                let Some(local) = word(0) else { return };
                let Some(channel) = self.channel_mut(local) else {
                    out.push(command(COMMAND_REJECT, ident, &le(&[0x0002, local, 0])));
                    return;
                };
                let remote = channel.remote;
                let mut options = data.get(4..).unwrap_or_default();
                let mut unknown = Vec::new();
                while options.len() >= 2 {
                    let (kind, len) = (options[0], usize::from(options[1]));
                    let Some(value) = options.get(2..2 + len) else { break };
                    match kind & !OPTION_HINT {
                        OPTION_MTU if len == 2 => {
                            channel.peer_mtu = u16::from_le_bytes([value[0], value[1]]);
                        }
                        // Flush timeout and quality of service are fine as asked.
                        0x02 | 0x03 => {}
                        // Basic mode is all there is, so any other mode is turned down.
                        _ if kind & OPTION_HINT == 0 => unknown.push(kind),
                        _ => {}
                    }
                    options = &options[2 + len..];
                }
                if unknown.is_empty() {
                    out.push(command(CONFIGURE_RESPONSE, ident, &le(&[remote, 0, RESULT_SUCCESS])));
                    signals.extend(self.configured(local, false));
                } else {
                    let mut reply = le(&[remote, 0, CONFIG_UNKNOWN_OPTIONS]);
                    reply.extend(unknown);
                    out.push(command(CONFIGURE_RESPONSE, ident, &reply));
                }
            }
            CONFIGURE_RESPONSE => {
                let (Some(local), Some(result)) = (word(0), word(4)) else { return };
                if result == RESULT_SUCCESS {
                    signals.extend(self.configured(local, true));
                } else if let Some(bye) = self.disconnect(local) {
                    out.push(bye);
                    signals.push(Signal::Refused { local });
                }
            }
            DISCONNECTION_REQUEST => {
                let (Some(local), Some(remote)) = (word(0), word(2)) else { return };
                out.push(command(DISCONNECTION_RESPONSE, ident, &le(&[local, remote])));
                if self.channel(local).is_some() {
                    self.channels.retain(|c| c.local != local);
                    signals.push(Signal::Closed { local });
                }
            }
            DISCONNECTION_RESPONSE => {}
            ECHO_REQUEST => out.push(command(ECHO_RESPONSE, ident, data)),
            INFORMATION_REQUEST => {
                let Some(kind) = word(0) else { return };
                let reply = if kind == INFO_EXTENDED_FEATURES {
                    [le(&[kind, RESULT_SUCCESS]), 0u32.to_le_bytes().to_vec()].concat()
                } else {
                    le(&[kind, INFO_NOT_SUPPORTED])
                };
                out.push(command(INFORMATION_RESPONSE, ident, &reply));
            }
            COMMAND_REJECT | ECHO_RESPONSE | INFORMATION_RESPONSE => {}
            _ => out.push(command(COMMAND_REJECT, ident, &le(&[0x0000]))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(link: &mut Link, cmds: &[u8]) -> (Vec<Vec<u8>>, Vec<Signal>) {
        link.signal(cmds, |psm| psm == PSM_RFCOMM)
    }

    fn sig(code: u8, ident: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![code, ident];
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn a_pdu_split_across_acl_packets_is_joined_again() {
        let whole = pdu(0x0040, &[1u8; 30]);
        let parts = split(0x002a, &whole, 16);
        assert_eq!(parts.len(), 3);
        let mut joiner = Joiner::default();
        let mut got = None;
        for p in &parts {
            let (_, start, data) = hci::read_acl(p).unwrap();
            got = joiner.push(start, data).or(got);
        }
        assert_eq!(got, Some((0x0040, vec![1u8; 30])));
        assert_eq!(joiner.push(false, &[1, 2]), None);
    }

    #[test]
    fn a_phone_opens_a_channel_and_both_sides_configure_it() {
        let mut link = Link::default();
        let (out, s) = signals(&mut link, &sig(CONNECTION_REQUEST, 7, &le(&[PSM_RFCOMM, 0x0041])));
        assert!(s.is_empty());
        assert_eq!(out[0][4..], sig(CONNECTION_RESPONSE, 7, &le(&[0x0040, 0x0041, 0, 0]))[..]);
        assert_eq!(out[1][4], CONFIGURE_REQUEST);
        let mut asked = le(&[0x0040, 0]);
        asked.extend_from_slice(&[OPTION_MTU, 2, 0x00, 0x04]);
        let (out, s) = signals(&mut link, &sig(CONFIGURE_REQUEST, 8, &asked));
        assert_eq!(out[0][4..], sig(CONFIGURE_RESPONSE, 8, &le(&[0x0041, 0, 0]))[..]);
        assert!(s.is_empty());
        let (_, s) = signals(&mut link, &sig(CONFIGURE_RESPONSE, 1, &le(&[0x0040, 0, 0])));
        assert_eq!(s, vec![Signal::Opened { local: 0x0040, psm: PSM_RFCOMM, ours: false }]);
        assert_eq!(link.channel(0x0040).unwrap().peer_mtu, 1024);
        let (out, s) = signals(&mut link, &sig(DISCONNECTION_REQUEST, 9, &le(&[0x0040, 0x0041])));
        assert_eq!(out[0][4..], sig(DISCONNECTION_RESPONSE, 9, &le(&[0x0040, 0x0041]))[..]);
        assert_eq!(s, vec![Signal::Closed { local: 0x0040 }]);
    }

    #[test]
    fn a_service_nobody_offers_is_turned_down() {
        let mut link = Link::default();
        let (out, _) = signals(&mut link, &sig(CONNECTION_REQUEST, 3, &le(&[0x0019, 0x0041])));
        assert_eq!(
            out[0][4..],
            sig(CONNECTION_RESPONSE, 3, &le(&[0, 0x0041, RESULT_NO_PSM, 0]))[..]
        );
        assert!(link.channels.is_empty());
    }

    #[test]
    fn our_own_channel_opens_once_the_phone_answers() {
        let mut link = Link::default();
        let (local, ask) = link.connect(PSM_SDP);
        assert_eq!(ask[4], CONNECTION_REQUEST);
        let (out, _) =
            signals(&mut link, &sig(CONNECTION_RESPONSE, 1, &le(&[0x0050, local, 1, 0])));
        assert!(out.is_empty());
        let (out, _) =
            signals(&mut link, &sig(CONNECTION_RESPONSE, 1, &le(&[0x0050, local, 0, 0])));
        assert_eq!(out[0][4], CONFIGURE_REQUEST);
        let (_, s) = signals(&mut link, &sig(CONFIGURE_RESPONSE, 2, &le(&[local, 0, 0])));
        assert!(s.is_empty());
        let (_, s) = signals(&mut link, &sig(CONFIGURE_REQUEST, 4, &le(&[local, 0])));
        assert_eq!(s, vec![Signal::Opened { local, psm: PSM_SDP, ours: true }]);
        assert_eq!(link.channel(local).unwrap().remote, 0x0050);
    }

    #[test]
    fn several_commands_in_one_pdu_are_all_answered() {
        let mut link = Link::default();
        let two =
            [sig(ECHO_REQUEST, 1, &[5]), sig(INFORMATION_REQUEST, 2, &le(&[0x0003]))].concat();
        let (out, _) = signals(&mut link, &two);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][4..], sig(ECHO_RESPONSE, 1, &[5])[..]);
        assert_eq!(out[1][4..], sig(INFORMATION_RESPONSE, 2, &le(&[0x0003, 1]))[..]);
    }
}
