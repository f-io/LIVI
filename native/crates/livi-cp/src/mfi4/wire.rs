// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const HEADER: usize = 6;
const PARAM_HEADER: usize = 4;

pub const CLEAR: u16 = 0x0000;
pub const TUNNELED: u16 = 0x4040;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub start: u16,
    pub id: u16,
    pub params: Vec<(u16, Vec<u8>)>,
}

impl Message {
    pub fn new(start: u16, id: u16) -> Self {
        Self {
            start,
            id,
            params: Vec::new(),
        }
    }

    pub fn with(mut self, id: u16, value: impl Into<Vec<u8>>) -> Self {
        self.params.push((id, value.into()));
        self
    }

    pub fn param(&self, id: u16) -> Option<&[u8]> {
        self.params
            .iter()
            .find(|(p, _)| *p == id)
            .map(|(_, v)| v.as_slice())
    }

    pub fn all(&self, id: u16) -> impl Iterator<Item = &[u8]> {
        self.params
            .iter()
            .filter(move |(p, _)| *p == id)
            .map(|(_, v)| v.as_slice())
    }

    pub fn u8(&self, id: u16) -> Option<u8> {
        match self.param(id)? {
            [v] => Some(*v),
            _ => None,
        }
    }

    pub fn u16(&self, id: u16) -> Option<u16> {
        self.param(id)?.try_into().ok().map(u16::from_be_bytes)
    }

    pub fn exact<const N: usize>(&self, id: u16) -> Option<[u8; N]> {
        self.param(id)?.try_into().ok()
    }

    pub fn ids(&self) -> Vec<u16> {
        self.params.iter().map(|(p, _)| *p).collect()
    }

    pub fn encode_params(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, value) in &self.params {
            out.extend_from_slice(&((value.len() + PARAM_HEADER) as u16).to_be_bytes());
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(value);
        }
        out
    }

    pub fn header(&self, body_len: usize) -> [u8; HEADER] {
        let mut h = [0; HEADER];
        h[..2].copy_from_slice(&self.start.to_be_bytes());
        h[2..4].copy_from_slice(&((HEADER + body_len) as u16).to_be_bytes());
        h[4..].copy_from_slice(&self.id.to_be_bytes());
        h
    }

    pub fn encode(&self) -> Vec<u8> {
        let params = self.encode_params();
        [&self.header(params.len())[..], &params].concat()
    }
}

pub fn peek(buf: &[u8]) -> Option<(u16, u16)> {
    if buf.len() < HEADER {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    (length == buf.len()).then(|| {
        (
            u16::from_be_bytes([buf[0], buf[1]]),
            u16::from_be_bytes([buf[4], buf[5]]),
        )
    })
}

pub fn decode(buf: &[u8]) -> Result<Message, String> {
    let (start, id) = peek(buf).ok_or("not one whole message")?;
    Ok(Message {
        start,
        id,
        params: decode_params(&buf[HEADER..])?,
    })
}

pub fn decode_params(mut buf: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, String> {
    let mut params = Vec::new();
    while !buf.is_empty() {
        if buf.len() < PARAM_HEADER {
            return Err("a parameter cut short".into());
        }
        let length = usize::from(u16::from_be_bytes([buf[0], buf[1]]));
        if length < PARAM_HEADER || length > buf.len() {
            return Err(format!(
                "a parameter of length {length} in {} bytes",
                buf.len()
            ));
        }
        params.push((
            u16::from_be_bytes([buf[2], buf[3]]),
            buf[PARAM_HEADER..length].to_vec(),
        ));
        buf = &buf[length..];
    }
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_goes_out_and_comes_back() {
        let msg = Message::new(CLEAR, 0x5201)
            .with(0, [3])
            .with(3, 0x1234u16.to_be_bytes());
        let bytes = msg.encode();
        assert_eq!(
            bytes,
            [
                0, 0, 0, 17, 0x52, 0x01, 0, 5, 0, 0, 3, 0, 6, 0, 3, 0x12, 0x34
            ]
        );
        let back = decode(&bytes).unwrap();
        assert_eq!(back, msg);
        assert_eq!(back.u16(3), Some(0x1234));
        assert_eq!(back.u8(0), Some(3));
    }

    #[test]
    fn a_length_that_does_not_fit_is_refused() {
        let mut bytes = Message::new(CLEAR, 1).with(0, [1, 2]).encode();
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        bytes[7] = 9;
        assert!(decode(&bytes).is_err());
    }
}
