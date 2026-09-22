// iAP2 link layer as a sans-io engine: bytes and a clock in, writes and events out.

use std::collections::VecDeque;

pub const CONTROL_SESSION_ID: u8 = 10;
pub const EA_SESSION_ID: u8 = 11;
pub const FILE_TRANSFER_SESSION_ID: u8 = 12;

pub const IAP2_MARKER: [u8; 6] = [0xFF, 0x55, 0x02, 0x00, 0xEE, 0x10];

const LINK_START: u16 = 0xFF5A;
const CONTROL_SYN: u8 = 0x80;
const CONTROL_ACK: u8 = 0x40;
const CONTROL_EAK: u8 = 0x20;
const CONTROL_RST: u8 = 0x10;

const DETECT_RESEND_MS: u64 = 1000;
const NEGOTIATE_RESEND_MS: u64 = 500;

pub fn gen_checksum(data: &[u8]) -> u8 {
    let sum = data.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    sum.wrapping_neg()
}

pub fn check_checksum(data: &[u8]) -> bool {
    data.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

fn distance(a: u8, b: Option<u8>) -> u16 {
    match b {
        None => 0,
        Some(b) => a.wrapping_sub(b) as u16,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkPacketHeader {
    pub length: u16,
    pub control: u8,
    pub seq: u8,
    pub ack: u8,
    pub session_id: u8,
}

impl LinkPacketHeader {
    pub fn parse(bytes: &[u8; 9]) -> Option<Self> {
        if !check_checksum(bytes) {
            return None;
        }
        if u16::from_be_bytes([bytes[0], bytes[1]]) != LINK_START {
            return None;
        }
        Some(Self {
            length: u16::from_be_bytes([bytes[2], bytes[3]]),
            control: bytes[4],
            seq: bytes[5],
            ack: bytes[6],
            session_id: bytes[7],
        })
    }

    pub fn pack(&self) -> [u8; 9] {
        let mut out = [0u8; 9];
        out[..2].copy_from_slice(&LINK_START.to_be_bytes());
        out[2..4].copy_from_slice(&self.length.to_be_bytes());
        out[4] = self.control;
        out[5] = self.seq;
        out[6] = self.ack;
        out[7] = self.session_id;
        out[8] = gen_checksum(&out[..8]);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspSession {
    pub id: u8,
    pub kind: u8,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSynchronizationPayload {
    pub max_outgoing: u8,
    pub max_len: u16,
    pub retransmission_timeout: u16,
    pub ack_timeout: u16,
    pub max_retransmissions: u8,
    pub max_ack: u8,
    pub sessions: Vec<LspSession>,
}

impl LinkSynchronizationPayload {
    const VERSION: u8 = 0x01;

    pub fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < 10 || payload[0] != Self::VERSION {
            return None;
        }
        let sessions = payload[10..]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| LspSession { id: c[0], kind: c[1], version: c[2] })
            .collect();
        Some(Self {
            max_outgoing: payload[1],
            max_len: u16::from_be_bytes([payload[2], payload[3]]),
            retransmission_timeout: u16::from_be_bytes([payload[4], payload[5]]),
            ack_timeout: u16::from_be_bytes([payload[6], payload[7]]),
            max_retransmissions: payload[8],
            max_ack: payload[9],
            sessions,
        })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut out = vec![Self::VERSION, self.max_outgoing];
        out.extend_from_slice(&self.max_len.to_be_bytes());
        out.extend_from_slice(&self.retransmission_timeout.to_be_bytes());
        out.extend_from_slice(&self.ack_timeout.to_be_bytes());
        out.push(self.max_retransmissions);
        out.push(self.max_ack);
        for s in &self.sessions {
            out.extend_from_slice(&[s.id, s.kind, s.version]);
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Idle,
    DetectIap2Support,
    Negotiate,
    Normal,
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Control(Vec<u8>),
    Ea { stream_id: u16, data: Vec<u8> },
    FileTransfer(Vec<u8>),
    Writable(bool),
    Dead { reason: Option<String> },
}

#[derive(Debug, Clone)]
pub struct LinkConfig {
    pub max_outgoing: u8,
    pub max_outgoing_delta: u16,
    pub ack_timeout_ms: u16,
    pub zero_ack: bool,
    pub control_version: u8,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            max_outgoing: 30,
            max_outgoing_delta: 0,
            ack_timeout_ms: 500,
            zero_ack: false,
            control_version: 1,
        }
    }
}

#[derive(Debug, Clone)]
struct Packet {
    psn: u8,
    session_id: u8,
    data: Vec<u8>,
    counter: u8,
    timeout: u64,
}

#[derive(Debug)]
enum RxState {
    Marker,
    Header,
    Payload(LinkPacketHeader),
}

pub struct LinkEngine {
    state: LinkState,
    pub lsp: LinkSynchronizationPayload,
    max_outgoing_delta: u16,

    sent_psn: u8,
    last_sent_acknowledged_psn: Option<u8>,
    unack_packets: Vec<Packet>,
    queued_packets: VecDeque<Packet>,

    last_received_in_sequence_psn: u8,
    last_acked_psn: Option<u8>,
    received_out_of_sequence: Vec<Packet>,
    cumulative_received: u32,

    writable: bool,
    rx_state: RxState,
    rx_buf: Vec<u8>,
    eof: bool,

    detect_deadline: Option<u64>,
    negotiate_deadline: Option<u64>,
    send_ack_deadline: Option<u64>,
    recv_ack_deadline: Option<u64>,

    output: Vec<u8>,
    events: VecDeque<Event>,
}

impl LinkEngine {
    pub fn new(cfg: LinkConfig) -> Self {
        let z = cfg.zero_ack;
        Self {
            state: LinkState::Idle,
            lsp: LinkSynchronizationPayload {
                max_outgoing: cfg.max_outgoing,
                max_len: 65535,
                retransmission_timeout: if z { 0 } else { 4000 },
                ack_timeout: if z { 0 } else { cfg.ack_timeout_ms },
                max_retransmissions: if z { 0 } else { 4 },
                max_ack: if z { 0 } else { 3 },
                sessions: vec![
                    LspSession { id: CONTROL_SESSION_ID, kind: 0, version: cfg.control_version },
                    LspSession { id: EA_SESSION_ID, kind: 2, version: 1 },
                    LspSession { id: FILE_TRANSFER_SESSION_ID, kind: 1, version: 2 },
                ],
            },
            max_outgoing_delta: cfg.max_outgoing_delta,
            sent_psn: 99,
            last_sent_acknowledged_psn: None,
            unack_packets: Vec::new(),
            queued_packets: VecDeque::new(),
            last_received_in_sequence_psn: 0,
            last_acked_psn: None,
            received_out_of_sequence: Vec::new(),
            cumulative_received: 0,
            writable: false,
            rx_state: RxState::Marker,
            rx_buf: Vec::new(),
            eof: false,
            detect_deadline: None,
            negotiate_deadline: None,
            send_ack_deadline: None,
            recv_ack_deadline: None,
            output: Vec::new(),
            events: VecDeque::new(),
        }
    }

    pub fn state(&self) -> LinkState {
        self.state
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn next_deadline(&self) -> Option<u64> {
        [
            self.detect_deadline,
            self.negotiate_deadline,
            self.send_ack_deadline,
            self.recv_ack_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn start(&mut self, initiate_negotiate: bool, now: u64) {
        if self.state != LinkState::Idle {
            return;
        }
        self.state = LinkState::DetectIap2Support;
        self.output.extend_from_slice(&IAP2_MARKER);
        self.detect_deadline = Some(now + DETECT_RESEND_MS);
        if initiate_negotiate {
            self.rx_state = RxState::Header;
            self.enter_negotiate(now);
        }
    }

    pub fn feed_eof(&mut self) {
        self.eof = true;
        self.bailout(None);
    }

    pub fn feed(&mut self, data: &[u8], now: u64) {
        if self.state == LinkState::Dead {
            return;
        }
        self.rx_buf.extend_from_slice(data);
        loop {
            match &self.rx_state {
                RxState::Marker => {
                    if self.rx_buf.len() < IAP2_MARKER.len() {
                        return;
                    }
                    if self.rx_buf[..IAP2_MARKER.len()] != IAP2_MARKER {
                        self.bailout(Some("IAP2 not supported".into()));
                        return;
                    }
                    self.rx_buf.drain(..IAP2_MARKER.len());
                    self.rx_state = RxState::Header;
                    self.enter_negotiate(now);
                }
                RxState::Header => {
                    while self.rx_buf.len() >= 2
                        && u16::from_be_bytes([self.rx_buf[0], self.rx_buf[1]]) != LINK_START
                    {
                        self.rx_buf.remove(0);
                    }
                    if self.rx_buf.len() < 9 {
                        return;
                    }
                    let header_bytes: [u8; 9] = self.rx_buf[..9].try_into().unwrap();
                    self.rx_buf.drain(..9);
                    let Some(header) = LinkPacketHeader::parse(&header_bytes) else {
                        continue;
                    };
                    if header.length > 9 {
                        self.rx_state = RxState::Payload(header);
                    } else {
                        self.process_frame(header, None, now);
                        if self.state == LinkState::Dead {
                            return;
                        }
                    }
                }
                RxState::Payload(header) => {
                    let header = *header;
                    let need = header.length as usize - 9;
                    if self.rx_buf.len() < need {
                        return;
                    }
                    let payload_with_checksum: Vec<u8> = self.rx_buf.drain(..need).collect();
                    self.rx_state = RxState::Header;
                    if !check_checksum(&payload_with_checksum) {
                        continue;
                    }
                    let payload = &payload_with_checksum[..payload_with_checksum.len() - 1];
                    self.process_frame(header, Some(payload.to_vec()), now);
                    if self.state == LinkState::Dead {
                        return;
                    }
                }
            }
        }
    }

    pub fn advance_time(&mut self, now: u64) {
        loop {
            let Some(deadline) = self.next_deadline() else { return };
            if deadline > now {
                return;
            }
            if self.detect_deadline.is_some_and(|d| d <= now) {
                self.detect_deadline = None;
                if self.state == LinkState::DetectIap2Support {
                    self.output.extend_from_slice(&IAP2_MARKER);
                    self.detect_deadline = Some(now + DETECT_RESEND_MS);
                }
            } else if self.negotiate_deadline.is_some_and(|d| d <= now) {
                self.negotiate_deadline = None;
                if self.state == LinkState::Negotiate {
                    self.send_negotiate();
                    self.negotiate_deadline = Some(now + NEGOTIATE_RESEND_MS);
                }
            } else if self.send_ack_deadline.is_some_and(|d| d <= now) {
                self.send_ack_deadline = None;
                if self.state == LinkState::Normal {
                    self.last_acked_psn = Some(self.last_received_in_sequence_psn);
                    self.send_ack();
                }
            } else if self.recv_ack_deadline.is_some_and(|d| d <= now) {
                self.recv_ack_deadline = None;
                self.on_expect_ack_timer(now);
            }
        }
    }

    pub fn send(&mut self, session_id: u8, data: Vec<u8>, now: u64) {
        self.send_packet(
            Packet { psn: 0, session_id, data, counter: 0, timeout: 0 },
            now,
        );
    }

    pub fn send_ea(&mut self, stream_id: u16, data: &[u8], now: u64) {
        let mut framed = stream_id.to_be_bytes().to_vec();
        framed.extend_from_slice(data);
        self.send(EA_SESSION_ID, framed, now);
    }

    fn enter_negotiate(&mut self, now: u64) {
        self.state = LinkState::Negotiate;
        self.detect_deadline = None;
        self.send_negotiate();
        self.negotiate_deadline = Some(now + NEGOTIATE_RESEND_MS);
    }

    fn write_packet(&mut self, payload: Option<&[u8]>, seq: u8, control: u8, session_id: u8) {
        if self.state == LinkState::Dead {
            return;
        }
        self.cumulative_received = 0;
        let length = match payload {
            Some(p) => p.len() as u16 + 10,
            None => 9,
        };
        let header = LinkPacketHeader {
            length,
            control,
            seq,
            ack: self.last_received_in_sequence_psn,
            session_id,
        };
        self.output.extend_from_slice(&header.pack());
        if let Some(p) = payload {
            self.output.extend_from_slice(p);
            self.output.push(gen_checksum(p));
        }
    }

    fn send_ack(&mut self) {
        self.write_packet(None, self.sent_psn, CONTROL_ACK, 0);
    }

    fn send_eak(&mut self, missing: &[u8]) {
        self.write_packet(Some(missing), self.sent_psn, CONTROL_EAK, 0);
    }

    fn send_data(&mut self, psn: u8, session_id: u8, data: &[u8]) {
        self.write_packet(Some(data), psn, CONTROL_ACK, session_id);
    }

    fn send_negotiate(&mut self) {
        let lsp_bytes = self.lsp.pack();
        self.write_packet(Some(&lsp_bytes), self.sent_psn, CONTROL_SYN, 0);
    }

    fn set_writable(&mut self, writable: bool) {
        if self.writable != writable {
            self.writable = writable;
            self.events.push_back(Event::Writable(writable));
        }
    }

    fn bailout(&mut self, reason: Option<String>) {
        if self.state == LinkState::Dead {
            return;
        }
        self.send_ack_deadline = None;
        self.recv_ack_deadline = None;
        self.detect_deadline = None;
        self.negotiate_deadline = None;
        self.state = LinkState::Dead;
        self.set_writable(false);
        self.events.push_back(Event::Dead { reason });
    }

    fn send_packet(&mut self, mut p: Packet, now: u64) {
        if distance(self.sent_psn, self.last_sent_acknowledged_psn) > self.lsp.max_outgoing as u16
            || self.state != LinkState::Normal
        {
            self.queued_packets.push_back(p);
            self.set_writable(false);
            return;
        }

        self.sent_psn = self.sent_psn.wrapping_add(1);
        p.counter = 0;
        p.psn = self.sent_psn;
        p.timeout = now + self.lsp.retransmission_timeout as u64;
        self.send_ack_deadline = None;
        self.send_data(p.psn, p.session_id, &p.data);
        self.last_acked_psn = Some(self.last_received_in_sequence_psn);
        if self.lsp.max_retransmissions > 0 {
            self.recv_ack_deadline = Some(p.timeout);
            self.unack_packets.push(p);
        } else {
            self.last_sent_acknowledged_psn = Some(self.sent_psn);
        }
    }

    fn process_frame(&mut self, header: LinkPacketHeader, payload: Option<Vec<u8>>, now: u64) {
        if header.control & CONTROL_RST != 0 {
            self.bailout(Some("device sent reset message".into()));
        }
        if header.control & CONTROL_SYN != 0 {
            let Some(lsp) = payload.as_deref().and_then(LinkSynchronizationPayload::parse) else {
                return;
            };
            self.handle_syn(lsp, header.seq);
        }
        if header.control & CONTROL_ACK != 0 {
            self.cumulative_received += 1;
            self.handle_ack(header.ack, now);
        }
        if header.control & CONTROL_EAK != 0
            && let Some(p) = &payload {
                self.handle_eak(p.clone());
            }
        if header.control & !CONTROL_ACK == 0
            && let Some(data) = payload {
                self.handle_data(
                    Packet {
                        psn: header.seq,
                        session_id: header.session_id,
                        data,
                        counter: 0,
                        timeout: 0,
                    },
                    now,
                );
            }
        if self.lsp.max_ack > 0 && self.cumulative_received >= self.lsp.max_ack as u32 {
            self.cumulative_received = 0;
            self.last_acked_psn = Some(self.last_received_in_sequence_psn);
            self.send_ack();
        }
    }

    fn handle_syn(&mut self, lsp: LinkSynchronizationPayload, psn: u8) {
        if self.state != LinkState::Negotiate {
            return;
        }
        self.lsp = lsp;
        self.last_received_in_sequence_psn = psn;
        self.last_acked_psn = Some(psn);
        self.send_ack();
    }

    fn handle_ack(&mut self, num: u8, now: u64) {
        if self.state == LinkState::Negotiate {
            self.state = LinkState::Normal;
            self.negotiate_deadline = None;
            self.set_writable(true);
        }
        self.last_sent_acknowledged_psn = Some(num);

        let mut rearmed = false;
        while let Some(first) = self.unack_packets.first() {
            let d = distance(first.psn, self.last_sent_acknowledged_psn);
            if d > 0 && d <= self.lsp.max_ack as u16 + 10 {
                self.recv_ack_deadline = Some(first.timeout);
                rearmed = true;
                break;
            }
            self.unack_packets.remove(0);
        }
        if !rearmed {
            self.recv_ack_deadline = None;
        }

        while distance(self.sent_psn, self.last_sent_acknowledged_psn)
            < self.lsp.max_outgoing as u16
        {
            let Some(p) = self.queued_packets.pop_front() else { break };
            self.send_packet(p, now);
            self.set_writable(true);
        }
    }

    fn on_expect_ack_timer(&mut self, now: u64) {
        if self.unack_packets.is_empty() || self.state != LinkState::Normal {
            return;
        }
        let mut order: Vec<usize> = (0..self.unack_packets.len()).collect();
        order.sort_by_key(|&i| self.unack_packets[i].timeout);
        let second_timeout = order.get(1).map(|&i| self.unack_packets[i].timeout);
        let first = order[0];
        self.unack_packets[first].timeout = now + self.lsp.retransmission_timeout as u64;
        self.unack_packets[first].counter += 1;
        let (psn, session_id, data, counter, timeout) = {
            let p = &self.unack_packets[first];
            (p.psn, p.session_id, p.data.clone(), p.counter, p.timeout)
        };
        if counter == self.lsp.max_retransmissions {
            self.bailout(Some(format!("unacknowledged packet psn={psn}")));
            return;
        }
        self.send_data(psn, session_id, &data);
        self.recv_ack_deadline = Some(second_timeout.unwrap_or(timeout));
    }

    fn handle_eak(&mut self, missing: Vec<u8>) {
        if self.state != LinkState::Normal {
            return;
        }
        for i in 0..self.unack_packets.len() {
            if !missing.contains(&self.unack_packets[i].psn) {
                continue;
            }
            self.unack_packets[i].counter += 1;
            if self.unack_packets[i].counter == self.lsp.max_retransmissions {
                let psn = self.unack_packets[i].psn;
                self.bailout(Some(format!("unacknowledged packet psn={psn}")));
                continue;
            }
            let (psn, session_id, data, timeout) = {
                let p = &self.unack_packets[i];
                (p.psn, p.session_id, p.data.clone(), p.timeout)
            };
            self.send_data(psn, session_id, &data);
            self.send_ack_deadline = None;
            self.recv_ack_deadline = Some(timeout);
        }
    }

    fn handle_data(&mut self, p: Packet, now: u64) {
        let d = distance(p.psn, Some(self.last_received_in_sequence_psn));
        if d > self.lsp.max_outgoing as u16 + 10 || d == 0 {
            self.send_ack();
            return;
        }

        if d > 1 {
            let trigger_psn = p.psn;
            self.received_out_of_sequence.push(p);
            if d >= self.lsp.max_outgoing as u16 {
                let mut eak = Vec::new();
                let mut x = self.last_received_in_sequence_psn;
                while distance(trigger_psn, Some(x)) > 1 {
                    x = x.wrapping_add(1);
                    eak.push(x);
                }
                self.send_ack_deadline = None;
                self.send_eak(&eak);
            }
            return;
        }

        self.received_out_of_sequence.push(p);
        self.received_out_of_sequence
            .sort_by_key(|x| distance(x.psn, Some(self.last_received_in_sequence_psn)));
        while !self.received_out_of_sequence.is_empty() {
            let d = distance(
                self.received_out_of_sequence[0].psn,
                Some(self.last_received_in_sequence_psn),
            );
            if d > 1 {
                break;
            }
            let pp = self.received_out_of_sequence.remove(0);
            self.last_received_in_sequence_psn = pp.psn;
            self.deliver(pp);
        }

        if self.lsp.max_ack == 0 {
            return;
        }
        if distance(self.last_received_in_sequence_psn, self.last_acked_psn)
            >= self.lsp.max_outgoing as u16 - self.max_outgoing_delta
        {
            self.send_ack_deadline = None;
            self.last_acked_psn = Some(self.last_received_in_sequence_psn);
            self.send_ack();
        } else {
            self.send_ack_deadline = Some(now + self.lsp.ack_timeout as u64);
        }
    }

    fn deliver(&mut self, p: Packet) {
        match p.session_id {
            CONTROL_SESSION_ID => self.events.push_back(Event::Control(p.data)),
            EA_SESSION_ID if p.data.len() >= 2 => {
                let stream_id = u16::from_be_bytes([p.data[0], p.data[1]]);
                self.events.push_back(Event::Ea { stream_id, data: p.data[2..].to_vec() });
            }
            FILE_TRANSFER_SESSION_ID => self.events.push_back(Event::FileTransfer(p.data)),
            _ => {}
        }
    }
}
