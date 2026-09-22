// The socket to the main process, one per session: four bytes of length in
// little endian, one byte of kind, then either an AA message
// `[ch][flags][msgId:u16 BE][payload]` or a line of JSON.

const LEN_BYTES: usize = 4;

pub const KIND_MESSAGE: u8 = 0;
pub const KIND_CONTROL: u8 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    Message { ch: u8, flags: u8, msg_id: u16, payload: Vec<u8> },
    Control(String),
}

pub fn encode_message(ch: u8, flags: u8, msg_id: u16, payload: &[u8]) -> Vec<u8> {
    let len = (1 + 4 + payload.len()) as u32;
    let mut out = Vec::with_capacity(LEN_BYTES + len as usize);
    out.extend_from_slice(&len.to_le_bytes());
    out.push(KIND_MESSAGE);
    out.push(ch);
    out.push(flags);
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn encode_control(json: &str) -> Vec<u8> {
    let len = (1 + json.len()) as u32;
    let mut out = Vec::with_capacity(LEN_BYTES + len as usize);
    out.extend_from_slice(&len.to_le_bytes());
    out.push(KIND_CONTROL);
    out.extend_from_slice(json.as_bytes());
    out
}

#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    pub fn next_item(&mut self) -> Option<Item> {
        loop {
            if self.buf.len() < LEN_BYTES {
                return None;
            }
            let len = u32::from_le_bytes(self.buf[..LEN_BYTES].try_into().unwrap()) as usize;
            if self.buf.len() < LEN_BYTES + len {
                return None;
            }
            let body = self.buf[LEN_BYTES..LEN_BYTES + len].to_vec();
            self.buf.drain(..LEN_BYTES + len);
            let Some((&kind, rest)) = body.split_first() else {
                continue;
            };
            match kind {
                KIND_MESSAGE if rest.len() >= 4 => {
                    return Some(Item::Message {
                        ch: rest[0],
                        flags: rest[1],
                        msg_id: u16::from_be_bytes([rest[2], rest[3]]),
                        payload: rest[4..].to_vec(),
                    });
                }
                KIND_CONTROL => return Some(Item::Control(String::from_utf8_lossy(rest).into_owned())),
                _ => continue,
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
            Some(Item::Message { ch: 3, flags: 0x0b, msg_id: 0x8004, payload: vec![8, 1] })
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
        assert_eq!(f.next_item(), Some(Item::Control("{\"type\":\"ready\"}".into())));
        assert_eq!(f.next_item(), Some(Item::Message { ch: 0, flags: 3, msg_id: 1, payload: vec![] }));
    }

    #[test]
    fn unknown_kinds_are_skipped() {
        let mut wire = vec![2u8, 0, 0, 0, 9, 0xFF];
        wire.extend(encode_message(1, 2, 3, &[4]));
        let mut f = Framer::default();
        f.push(&wire);
        assert_eq!(f.next_item().unwrap(), Item::Message { ch: 1, flags: 2, msg_id: 3, payload: vec![4] });
    }
}
