// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const DETECT_MARKER: [u8; 6] = [0xFF, 0x55, 0x02, 0x00, 0xEE, 0x10];

pub const HEADER_LEN: usize = 9;
pub(crate) const SOP: [u8; 2] = [0xFF, 0x5A];

pub const CONTROL_SYN: u8 = 0x80;
pub const CONTROL_ACK: u8 = 0x40;
pub const CONTROL_EAK: u8 = 0x20;
pub const CONTROL_RST: u8 = 0x10;

pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |sum, b| sum.wrapping_sub(*b))
}

pub fn checksum_ok(data: &[u8]) -> bool {
    data.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub length: u16,
    pub control: u8,
    pub seq: u8,
    pub ack: u8,
    pub session_id: u8,
}

impl FrameHeader {
    pub fn parse(bytes: &[u8; HEADER_LEN]) -> Option<Self> {
        if bytes[..2] != SOP || !checksum_ok(bytes) {
            return None;
        }
        let length = u16::from_be_bytes([bytes[2], bytes[3]]);
        if (length as usize) < HEADER_LEN {
            return None;
        }
        Some(Self {
            length,
            control: bytes[4],
            seq: bytes[5],
            ack: bytes[6],
            session_id: bytes[7],
        })
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let [l0, l1] = self.length.to_be_bytes();
        let mut out = [
            SOP[0],
            SOP[1],
            l0,
            l1,
            self.control,
            self.seq,
            self.ack,
            self.session_id,
            0,
        ];
        out[8] = checksum(&out[..8]);
        out
    }
}

pub(crate) fn encode(
    out: &mut Vec<u8>,
    control: u8,
    seq: u8,
    ack: u8,
    session_id: u8,
    payload: Option<&[u8]>,
) {
    let length = match payload {
        Some(p) => (HEADER_LEN + p.len() + 1) as u16,
        None => HEADER_LEN as u16,
    };
    let header = FrameHeader {
        length,
        control,
        seq,
        ack,
        session_id,
    };
    out.extend_from_slice(&header.to_bytes());
    if let Some(p) = payload {
        out.extend_from_slice(p);
        out.push(checksum(p));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u8,
    pub kind: u8,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncParams {
    pub max_outstanding: u8,
    pub max_packet_len: u16,
    pub retransmit_timeout_ms: u16,
    pub ack_timeout_ms: u16,
    pub max_retransmits: u8,
    pub max_cumulative_acks: u8,
    pub sessions: Vec<SessionInfo>,
}

impl SyncParams {
    const VERSION: u8 = 0x01;
    const FIXED_LEN: usize = 10;

    pub fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < Self::FIXED_LEN || payload[0] != Self::VERSION {
            return None;
        }
        let u16_at = |i: usize| u16::from_be_bytes([payload[i], payload[i + 1]]);
        Some(Self {
            max_outstanding: payload[1],
            max_packet_len: u16_at(2),
            retransmit_timeout_ms: u16_at(4),
            ack_timeout_ms: u16_at(6),
            max_retransmits: payload[8],
            max_cumulative_acks: payload[9],
            sessions: payload[Self::FIXED_LEN..]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|&[id, kind, version]| SessionInfo { id, kind, version })
                .collect(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::FIXED_LEN + 3 * self.sessions.len());
        out.extend_from_slice(&[Self::VERSION, self.max_outstanding]);
        out.extend_from_slice(&self.max_packet_len.to_be_bytes());
        out.extend_from_slice(&self.retransmit_timeout_ms.to_be_bytes());
        out.extend_from_slice(&self.ack_timeout_ms.to_be_bytes());
        out.extend_from_slice(&[self.max_retransmits, self.max_cumulative_acks]);
        for s in &self.sessions {
            out.extend_from_slice(&[s.id, s.kind, s.version]);
        }
        out
    }

    pub(crate) fn reliable(&self) -> bool {
        self.max_retransmits > 0 && self.retransmit_timeout_ms > 0
    }

    pub(crate) fn max_payload(&self) -> usize {
        (self.max_packet_len as usize)
            .saturating_sub(HEADER_LEN + 1)
            .max(1)
    }
}
