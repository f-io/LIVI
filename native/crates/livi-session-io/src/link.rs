// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_base::frame::{self, Frames, TooLarge};

pub const KIND_MESSAGE: u8 = 0;
pub const KIND_CONTROL: u8 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    Message {
        ch: u8,
        flags: u8,
        msg_id: u16,
        payload: Vec<u8>,
    },
    Control(String),
}

pub fn encode_message(ch: u8, flags: u8, msg_id: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = frame::open(1 + 4 + payload.len());
    out.push(KIND_MESSAGE);
    out.push(ch);
    out.push(flags);
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(payload);
    frame::seal(out)
}

pub fn encode_control(json: &str) -> Vec<u8> {
    let mut out = frame::open(1 + json.len());
    out.push(KIND_CONTROL);
    out.extend_from_slice(json.as_bytes());
    frame::seal(out)
}

#[derive(Default)]
pub struct Framer {
    frames: Frames,
}

impl Framer {
    pub fn push(&mut self, chunk: &[u8]) {
        self.frames.push(chunk);
    }

    pub fn next_item(&mut self) -> Option<Result<Item, TooLarge>> {
        loop {
            let item = self.frames.next_with(|body| match body.split_first()? {
                (&KIND_MESSAGE, &[ch, flags, a, b, ref payload @ ..]) => Some(Item::Message {
                    ch,
                    flags,
                    msg_id: u16::from_be_bytes([a, b]),
                    payload: payload.to_vec(),
                }),
                (&KIND_CONTROL, rest) => {
                    Some(Item::Control(String::from_utf8_lossy(rest).into_owned()))
                }
                _ => None,
            })?;
            if let Some(item) = item.transpose() {
                return Some(item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips() {
        let mut f = Framer::default();
        f.push(&encode_message(3, 0x0b, 0x8004, &[8, 1]));
        assert_eq!(
            f.next_item(),
            Some(Ok(Item::Message {
                ch: 3,
                flags: 0x0b,
                msg_id: 0x8004,
                payload: vec![8, 1]
            }))
        );
        assert_eq!(f.next_item(), None);
    }

    #[test]
    fn control_and_messages_keep_their_order() {
        let mut wire = encode_control("{\"type\":\"ready\"}");
        wire.extend(encode_message(0, 3, 1, &[]));
        let mut f = Framer::default();
        for piece in wire.chunks(3) {
            f.push(piece);
        }
        assert_eq!(
            f.next_item(),
            Some(Ok(Item::Control("{\"type\":\"ready\"}".into())))
        );
        assert_eq!(
            f.next_item(),
            Some(Ok(Item::Message {
                ch: 0,
                flags: 3,
                msg_id: 1,
                payload: vec![]
            }))
        );
    }

    #[test]
    fn unknown_kinds_are_skipped() {
        let mut wire = vec![2u8, 0, 0, 0, 9, 0xFF];
        wire.extend(encode_message(1, 2, 3, &[4]));
        let mut f = Framer::default();
        f.push(&wire);
        assert_eq!(
            f.next_item().unwrap().unwrap(),
            Item::Message {
                ch: 1,
                flags: 2,
                msg_id: 3,
                payload: vec![4]
            }
        );
    }
}
