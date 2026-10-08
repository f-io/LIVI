//! The control side of the microphone channel (9). The samples go from the
//! pipeline's tap to the helper.

use livi_aa_proto::{MediaStartNotification, MessageStatus, MicrophoneRequest, MicrophoneResponse};

use crate::channels::{Emit, Frame};
use crate::codec::{decode, encode};
use crate::consts::{frame_flags, media_msg};
use crate::log::{debug, detail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicEvent {
    Start,
    Stop,
}

pub type MicOut = Emit<MicEvent>;

#[derive(Debug)]
pub struct MicChannel {
    channel: u8,
    sample_rate: u32,
    channel_count: u32,
    /// Ours, the phone echoes it in its acks.
    session: i32,
    open: bool,
}

impl MicChannel {
    pub fn new(channel: u8) -> Self {
        Self { channel, sample_rate: 16000, channel_count: 1, session: 1, open: false }
    }

    pub fn handle_message(&mut self, msg_id: u16, payload: &[u8]) -> Vec<MicOut> {
        match msg_id {
            media_msg::SETUP | media_msg::ACK => Vec::new(),
            media_msg::MICROPHONE_REQUEST => self.on_open_request(payload),
            media_msg::STOP => {
                if self.open {
                    self.open = false;
                    detail!("[MicChannel] STOP, closing mic");
                    vec![Emit::Event(MicEvent::Stop)]
                } else {
                    Vec::new()
                }
            }
            other => {
                if debug() {
                    println!("[MicChannel] unhandled msgId=0x{other:x}");
                }
                Vec::new()
            }
        }
    }

    pub fn handle_setup_request(&mut self, codec: i32, sample_rate: u32, channel_count: u32) {
        if sample_rate != 0 {
            self.sample_rate = sample_rate;
        }
        if channel_count != 0 {
            self.channel_count = channel_count;
        }
        detail!("[MicChannel] setup codec={codec} {}Hz {}ch", self.sample_rate, self.channel_count);
    }

    pub fn format(&self) -> (u32, u32) {
        (self.sample_rate, self.channel_count)
    }

    fn on_open_request(&mut self, payload: &[u8]) -> Vec<MicOut> {
        let open = decode::<MicrophoneRequest>(payload, &[1]).is_ok_and(|r| r.open);
        detail!("[MicChannel] OPEN_REQUEST open={open}");
        let response = encode(&MicrophoneResponse {
            status: MessageStatus::Success as i32,
            session_id: Some(self.session),
        });
        let mut out = vec![Emit::Send(Frame::new(
            self.channel,
            frame_flags::ENC_SIGNAL,
            media_msg::MICROPHONE_RESPONSE,
            response,
        ))];
        if open && !self.open {
            self.open = true;
            let start = encode(&MediaStartNotification {
                session_id: self.session,
                configuration_index: 0,
                ..Default::default()
            });
            out.push(Emit::Send(Frame::new(
                self.channel,
                frame_flags::ENC_SIGNAL,
                media_msg::START,
                start,
            )));
            detail!("[MicChannel] mic open, session={}", self.session);
            out.push(Emit::Event(MicEvent::Start));
        } else if !open && self.open {
            self.open = false;
            detail!("[MicChannel] mic close");
            out.push(Emit::Event(MicEvent::Stop));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::ch;

    #[test]
    fn opening_announces_the_stream_once() {
        let mut m = MicChannel::new(ch::MIC_INPUT);
        let out = m.handle_message(media_msg::MICROPHONE_REQUEST, &[0x08, 0x01]);
        assert_eq!(
            out,
            [
                Emit::Send(Frame::new(
                    9,
                    0x0b,
                    media_msg::MICROPHONE_RESPONSE,
                    [0x08, 0x00, 0x10, 0x01]
                )),
                Emit::Send(Frame::new(9, 0x0b, media_msg::START, [0x08, 0x01, 0x10, 0x00])),
                Emit::Event(MicEvent::Start),
            ]
        );
        assert_eq!(m.handle_message(media_msg::MICROPHONE_REQUEST, &[0x08, 0x01]).len(), 1);
        assert_eq!(
            m.handle_message(media_msg::MICROPHONE_REQUEST, &[0x08, 0x00]).last(),
            Some(&Emit::Event(MicEvent::Stop))
        );
        assert!(m.handle_message(media_msg::STOP, &[]).is_empty());
        m.handle_message(media_msg::MICROPHONE_REQUEST, &[0x08, 0x01]);
        assert_eq!(m.handle_message(media_msg::STOP, &[]), [Emit::Event(MicEvent::Stop)]);
        assert!(m.handle_message(media_msg::ACK, &[]).is_empty());
        assert!(m.handle_message(media_msg::SETUP, &[]).is_empty());
        assert!(m.handle_message(0x4444, &[]).is_empty());
        m.handle_setup_request(1, 0, 0);
        assert_eq!(m.format(), (16000, 1));
        m.handle_setup_request(1, 48000, 2);
        assert_eq!(m.format(), (48000, 2));
    }
}
