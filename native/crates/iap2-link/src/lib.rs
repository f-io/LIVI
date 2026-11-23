// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod frame;

use std::collections::VecDeque;
use std::fmt;

pub use frame::{
    CONTROL_ACK, CONTROL_EAK, CONTROL_RST, CONTROL_SYN, DETECT_MARKER, FrameHeader, HEADER_LEN,
    SessionInfo, SyncParams, checksum, checksum_ok,
};

use frame::{SOP, encode};

pub const CONTROL_SESSION_ID: u8 = 10;
pub const EA_SESSION_ID: u8 = 11;
pub const FILE_TRANSFER_SESSION_ID: u8 = 12;

const DETECT_INTERVAL_MS: u64 = 1000;
const DETECT_TRIES: u8 = 30;
const SYN_INTERVAL_MS: u64 = 500;
const SYN_TRIES: u8 = 20;

const PSN_HALF: u8 = 128;

fn ahead(from: u8, to: u8) -> u8 {
    to.wrapping_sub(from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Idle,
    DetectIap2Support,
    Negotiate,
    Normal,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    NotIap2,
    NoMarker,
    NegotiationTimeout,
    Reset,
    Unacknowledged { psn: u8 },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIap2 => f.write_str("peer does not speak iAP2"),
            Self::NoMarker => f.write_str("no iAP2 marker from the peer"),
            Self::NegotiationTimeout => f.write_str("link synchronisation timed out"),
            Self::Reset => f.write_str("peer reset the link"),
            Self::Unacknowledged { psn } => write!(f, "packet {psn} never acknowledged"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Control(Vec<u8>),
    Ea { stream_id: u16, data: Vec<u8> },
    FileTransfer(Vec<u8>),
    Writable(bool),
    Dead { reason: Option<LinkError> },
}

#[derive(Debug, Clone)]
pub struct LinkConfig {
    pub max_outstanding: u8,
    pub max_packet_len: u16,
    pub retransmit_timeout_ms: u16,
    pub ack_timeout_ms: u16,
    pub max_retransmits: u8,
    pub max_cumulative_acks: u8,
    pub control_version: u8,
    pub zero_ack: bool,
    pub initial_psn: u8,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            max_outstanding: 4,
            max_packet_len: u16::MAX,
            retransmit_timeout_ms: 1500,
            ack_timeout_ms: 100,
            max_retransmits: 5,
            max_cumulative_acks: 2,
            control_version: 1,
            zero_ack: false,
            initial_psn: 0,
        }
    }
}

impl LinkConfig {
    fn params(&self) -> SyncParams {
        let z = self.zero_ack;
        SyncParams {
            max_outstanding: self.max_outstanding,
            max_packet_len: self.max_packet_len,
            retransmit_timeout_ms: if z { 0 } else { self.retransmit_timeout_ms },
            ack_timeout_ms: if z { 0 } else { self.ack_timeout_ms },
            max_retransmits: if z { 0 } else { self.max_retransmits },
            max_cumulative_acks: if z { 0 } else { self.max_cumulative_acks },
            sessions: vec![
                SessionInfo {
                    id: CONTROL_SESSION_ID,
                    kind: 0,
                    version: self.control_version,
                },
                SessionInfo {
                    id: EA_SESSION_ID,
                    kind: 2,
                    version: 1,
                },
                SessionInfo {
                    id: FILE_TRANSFER_SESSION_ID,
                    kind: 1,
                    version: 2,
                },
            ],
        }
    }
}

struct InFlight {
    psn: u8,
    session_id: u8,
    data: Vec<u8>,
    deadline: u64,
    retries: u8,
}

struct Received {
    psn: u8,
    session_id: u8,
    data: Vec<u8>,
}

#[derive(PartialEq, Eq)]
enum RxMode {
    Marker,
    Frames,
}

pub struct LinkEngine {
    state: LinkState,
    local: SyncParams,
    link: SyncParams,
    writable: bool,

    rx_mode: RxMode,
    rx_buf: Vec<u8>,

    tx_psn: u8,
    tx_acked: u8,
    in_flight: VecDeque<InFlight>,
    queue: VecDeque<(u8, Vec<u8>)>,

    rx_psn: u8,
    rx_synced: bool,
    rx_pending: Vec<Received>,
    rx_unacked: u8,
    ack_at: Option<u64>,

    detect_at: Option<u64>,
    detect_tries: u8,
    syn_at: Option<u64>,
    syn_tries: u8,

    output: Vec<u8>,
    events: VecDeque<Event>,
}

impl LinkEngine {
    pub fn new(cfg: LinkConfig) -> Self {
        let local = cfg.params();
        Self {
            state: LinkState::Idle,
            link: local.clone(),
            local,
            writable: false,
            rx_mode: RxMode::Marker,
            rx_buf: Vec::new(),
            tx_psn: cfg.initial_psn,
            tx_acked: cfg.initial_psn,
            in_flight: VecDeque::new(),
            queue: VecDeque::new(),
            rx_psn: 0,
            rx_synced: false,
            rx_pending: Vec::new(),
            rx_unacked: 0,
            ack_at: None,
            detect_at: None,
            detect_tries: 0,
            syn_at: None,
            syn_tries: 0,
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

    pub fn local_params(&self) -> &SyncParams {
        &self.local
    }

    pub fn link_params(&self) -> &SyncParams {
        &self.link
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn next_deadline(&self) -> Option<u64> {
        let retransmit = self.in_flight.iter().map(|p| p.deadline).min();
        [self.detect_at, self.syn_at, self.ack_at, retransmit]
            .into_iter()
            .flatten()
            .min()
    }

    pub fn start(&mut self, initiate_negotiate: bool, now: u64) {
        if self.state != LinkState::Idle {
            return;
        }
        self.state = LinkState::DetectIap2Support;
        self.output.extend_from_slice(&DETECT_MARKER);
        self.detect_at = Some(now + DETECT_INTERVAL_MS);
        if initiate_negotiate {
            self.rx_mode = RxMode::Frames;
            self.enter_negotiate(now);
        }
    }

    pub fn feed_eof(&mut self) {
        self.die(None);
    }

    pub fn feed(&mut self, data: &[u8], now: u64) {
        if self.state == LinkState::Dead {
            return;
        }
        self.rx_buf.extend_from_slice(data);
        let mut pos = 0;
        if self.rx_mode == RxMode::Marker {
            if self.rx_buf.len() < DETECT_MARKER.len() {
                return;
            }
            if self.rx_buf[..DETECT_MARKER.len()] != DETECT_MARKER {
                self.die(Some(LinkError::NotIap2));
                return;
            }
            pos = DETECT_MARKER.len();
            self.rx_mode = RxMode::Frames;
            self.enter_negotiate(now);
        }
        while let Some((consumed, frame)) = next_frame(&self.rx_buf[pos..]) {
            pos += consumed;
            if let Some((header, payload)) = frame {
                self.process(header, payload, now);
                if self.state == LinkState::Dead {
                    self.rx_buf.clear();
                    return;
                }
            }
        }
        self.rx_buf.drain(..pos);
    }

    pub fn advance_time(&mut self, now: u64) {
        if self.state == LinkState::Dead {
            return;
        }
        if self.detect_at.is_some_and(|t| t <= now) {
            if self.detect_tries >= DETECT_TRIES {
                self.die(Some(LinkError::NoMarker));
                return;
            }
            self.detect_tries += 1;
            self.output.extend_from_slice(&DETECT_MARKER);
            self.detect_at = Some(now + DETECT_INTERVAL_MS);
        }
        if self.syn_at.is_some_and(|t| t <= now) {
            if self.syn_tries >= SYN_TRIES {
                self.die(Some(LinkError::NegotiationTimeout));
                return;
            }
            self.syn_tries += 1;
            self.send_syn(now);
        }
        if self.ack_at.is_some_and(|t| t <= now) {
            self.send_ack();
        }
        self.retransmit(now, |p| p.deadline <= now);
    }

    fn retransmit(&mut self, now: u64, due: impl Fn(&InFlight) -> bool) {
        let rto = u64::from(self.link.retransmit_timeout_ms);
        let mut resent = false;
        let mut failed = None;
        for p in self.in_flight.iter_mut().filter(|p| due(p)) {
            if p.retries >= self.link.max_retransmits {
                failed = Some(p.psn);
                break;
            }
            p.retries += 1;
            p.deadline = now + rto;
            encode(
                &mut self.output,
                CONTROL_ACK,
                p.psn,
                self.rx_psn,
                p.session_id,
                Some(&p.data),
            );
            resent = true;
        }
        if let Some(psn) = failed {
            self.die(Some(LinkError::Unacknowledged { psn }));
        } else if resent {
            self.acked_now();
        }
    }

    pub fn send(&mut self, session_id: u8, data: Vec<u8>, now: u64) {
        if self.state == LinkState::Dead || data.is_empty() {
            return;
        }
        self.queue.push_back((session_id, data));
        self.flush(now);
        self.update_writable();
    }

    pub fn send_ea(&mut self, stream_id: u16, data: &[u8], now: u64) {
        let chunk = self.link.max_payload().saturating_sub(2).max(1);
        for part in data.chunks(chunk) {
            let mut framed = Vec::with_capacity(part.len() + 2);
            framed.extend_from_slice(&stream_id.to_be_bytes());
            framed.extend_from_slice(part);
            self.queue.push_back((EA_SESSION_ID, framed));
        }
        self.flush(now);
        self.update_writable();
    }

    fn enter_negotiate(&mut self, now: u64) {
        self.state = LinkState::Negotiate;
        self.detect_at = None;
        self.send_syn(now);
    }

    fn send_syn(&mut self, now: u64) {
        let params = self.local.to_bytes();
        encode(
            &mut self.output,
            CONTROL_SYN,
            self.tx_psn,
            self.rx_psn,
            0,
            Some(&params),
        );
        self.syn_at = Some(now + SYN_INTERVAL_MS);
    }

    fn send_ack(&mut self) {
        encode(
            &mut self.output,
            CONTROL_ACK,
            self.tx_psn,
            self.rx_psn,
            0,
            None,
        );
        self.acked_now();
    }

    fn acked_now(&mut self) {
        self.rx_unacked = 0;
        self.ack_at = None;
    }

    fn die(&mut self, reason: Option<LinkError>) {
        if self.state == LinkState::Dead {
            return;
        }
        self.state = LinkState::Dead;
        self.detect_at = None;
        self.syn_at = None;
        self.ack_at = None;
        self.in_flight.clear();
        self.queue.clear();
        self.update_writable();
        self.events.push_back(Event::Dead { reason });
    }

    fn update_writable(&mut self) {
        let writable = self.state == LinkState::Normal && self.queue.is_empty();
        if writable != self.writable {
            self.writable = writable;
            self.events.push_back(Event::Writable(writable));
        }
    }

    fn flush(&mut self, now: u64) {
        if self.state != LinkState::Normal {
            return;
        }
        let window = self.link.max_outstanding;
        let max_payload = self.link.max_payload();
        while ahead(self.tx_acked, self.tx_psn) < window {
            let Some((session_id, mut data)) = self.queue.pop_front() else {
                break;
            };
            if session_id != EA_SESSION_ID && data.len() > max_payload {
                let rest = data.split_off(max_payload);
                self.queue.push_front((session_id, rest));
            }
            self.transmit(session_id, data, now);
        }
    }

    fn transmit(&mut self, session_id: u8, data: Vec<u8>, now: u64) {
        self.tx_psn = self.tx_psn.wrapping_add(1);
        encode(
            &mut self.output,
            CONTROL_ACK,
            self.tx_psn,
            self.rx_psn,
            session_id,
            Some(&data),
        );
        self.acked_now();
        if self.link.reliable() {
            self.in_flight.push_back(InFlight {
                psn: self.tx_psn,
                session_id,
                data,
                deadline: now + u64::from(self.link.retransmit_timeout_ms),
                retries: 0,
            });
        } else {
            self.tx_acked = self.tx_psn;
        }
    }

    fn process(&mut self, header: FrameHeader, payload: Option<Vec<u8>>, now: u64) {
        if header.control & CONTROL_RST != 0 {
            self.die(Some(LinkError::Reset));
            return;
        }
        if header.control & CONTROL_SYN != 0 {
            if let Some(params) = payload.as_deref().and_then(SyncParams::parse) {
                self.handle_syn(params, header.seq);
            }
            if header.control & CONTROL_ACK != 0 {
                self.handle_ack(header.ack, now);
            }
            return;
        }
        if header.control & CONTROL_ACK != 0 {
            self.handle_ack(header.ack, now);
        }
        if header.control & CONTROL_EAK != 0 {
            if let Some(missing) = &payload {
                self.handle_eak(missing, now);
            }
            return;
        }
        if let Some(data) = payload
            && self.state == LinkState::Normal
        {
            self.handle_data(header.seq, header.session_id, data, now);
        }
    }

    fn handle_syn(&mut self, mut params: SyncParams, psn: u8) {
        // in Normal the peer still waits for our ACK of its SYN, either it got lost or the
        // peer's first SYN did and only our own SYN was acknowledged
        let adopt = match self.state {
            LinkState::Negotiate => true,
            LinkState::Normal if !self.rx_synced => true,
            LinkState::Normal if psn == self.rx_psn => false,
            _ => return,
        };
        if adopt {
            params.max_outstanding = params.max_outstanding.clamp(1, PSN_HALF - 1);
            self.link = params;
            self.rx_psn = psn;
            self.rx_synced = true;
        }
        self.send_ack();
    }

    fn handle_ack(&mut self, ack: u8, now: u64) {
        match self.state {
            LinkState::Negotiate if ack == self.tx_psn => {
                self.state = LinkState::Normal;
                self.syn_at = None;
                self.flush(now);
                self.update_writable();
            }
            LinkState::Normal => {
                let acked = ahead(self.tx_acked, ack);
                if acked == 0 || acked > ahead(self.tx_acked, self.tx_psn) {
                    return;
                }
                self.tx_acked = ack;
                let done = (acked as usize).min(self.in_flight.len());
                self.in_flight.drain(..done);
                self.flush(now);
                self.update_writable();
            }
            _ => {}
        }
    }

    fn handle_eak(&mut self, missing: &[u8], now: u64) {
        if self.state == LinkState::Normal {
            self.retransmit(now, |p| missing.contains(&p.psn));
        }
    }

    fn handle_data(&mut self, psn: u8, session_id: u8, data: Vec<u8>, now: u64) {
        if !self.rx_synced {
            self.rx_psn = psn.wrapping_sub(1);
            self.rx_synced = true;
        }
        let distance = ahead(self.rx_psn, psn);
        if distance == 0 || distance >= PSN_HALF || distance > self.link.max_outstanding {
            self.send_ack();
            return;
        }
        if distance > 1 {
            if self.rx_pending.iter().any(|r| r.psn == psn) {
                return;
            }
            let newest = self
                .rx_pending
                .iter()
                .map(|r| ahead(self.rx_psn, r.psn))
                .max()
                .unwrap_or(0);
            self.rx_pending.push(Received {
                psn,
                session_id,
                data,
            });
            // only a newly opened gap is reported, every EAK costs the sender a retry
            if distance > newest + 1 {
                self.send_eak(psn);
            }
            return;
        }

        self.deliver(session_id, data);
        self.rx_psn = psn;
        self.rx_unacked = self.rx_unacked.saturating_add(1);
        while let Some(i) = self
            .rx_pending
            .iter()
            .position(|r| r.psn == self.rx_psn.wrapping_add(1))
        {
            let r = self.rx_pending.swap_remove(i);
            self.rx_psn = r.psn;
            self.rx_unacked = self.rx_unacked.saturating_add(1);
            self.deliver(r.session_id, r.data);
        }
        let rx_psn = self.rx_psn;
        self.rx_pending
            .retain(|r| (1..PSN_HALF).contains(&ahead(rx_psn, r.psn)));

        if self.link.max_cumulative_acks == 0 {
            return;
        }
        if self.rx_unacked >= self.link.max_cumulative_acks || self.link.ack_timeout_ms == 0 {
            self.send_ack();
        } else if self.ack_at.is_none() {
            self.ack_at = Some(now + u64::from(self.link.ack_timeout_ms));
        }
    }

    fn send_eak(&mut self, newest: u8) {
        let missing: Vec<u8> = (1..ahead(self.rx_psn, newest))
            .map(|i| self.rx_psn.wrapping_add(i))
            .filter(|psn| !self.rx_pending.iter().any(|r| r.psn == *psn))
            .collect();
        encode(
            &mut self.output,
            CONTROL_ACK | CONTROL_EAK,
            self.tx_psn,
            self.rx_psn,
            0,
            Some(&missing),
        );
        self.acked_now();
    }

    fn deliver(&mut self, session_id: u8, data: Vec<u8>) {
        let event = match session_id {
            CONTROL_SESSION_ID => Event::Control(data),
            EA_SESSION_ID if data.len() >= 2 => Event::Ea {
                stream_id: u16::from_be_bytes([data[0], data[1]]),
                data: data[2..].to_vec(),
            },
            FILE_TRANSFER_SESSION_ID => Event::FileTransfer(data),
            _ => return,
        };
        self.events.push_back(event);
    }
}

type Frame = Option<(FrameHeader, Option<Vec<u8>>)>;

fn next_frame(buf: &[u8]) -> Option<(usize, Frame)> {
    let Some(start) = buf.windows(2).position(|w| w == SOP) else {
        let keep = usize::from(buf.last() == Some(&SOP[0]));
        return (buf.len() > keep).then(|| (buf.len() - keep, None));
    };
    if start > 0 {
        return Some((start, None));
    }
    let header_bytes: &[u8; HEADER_LEN] = buf.get(..HEADER_LEN)?.try_into().ok()?;
    let Some(header) = FrameHeader::parse(header_bytes) else {
        return Some((1, None));
    };
    let len = header.length as usize;
    let frame = buf.get(..len)?;
    if len == HEADER_LEN {
        return Some((len, Some((header, None))));
    }
    let body = &frame[HEADER_LEN..];
    if !checksum_ok(body) {
        return Some((len, None));
    }
    Some((len, Some((header, Some(body[..body.len() - 1].to_vec())))))
}
