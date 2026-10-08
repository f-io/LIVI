//! The samples go from the helper straight to the host.

use crate::consts::{ch, media_msg};
use crate::log::{debug, detail};
use crate::wire::decode_start;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioChannelType {
    Media,
    Speech,
    System,
    Telephony,
}

impl AudioChannelType {
    pub fn of_channel(channel: u8) -> Self {
        match channel {
            ch::SPEECH_AUDIO => Self::Speech,
            ch::SYSTEM_AUDIO => Self::System,
            ch::TELEPHONY_AUDIO => Self::Telephony,
            _ => Self::Media,
        }
    }

    pub fn channel(self) -> u8 {
        match self {
            Self::Media => ch::MEDIA_AUDIO,
            Self::Speech => ch::SPEECH_AUDIO,
            Self::System => ch::SYSTEM_AUDIO,
            Self::Telephony => ch::TELEPHONY_AUDIO,
        }
    }

    /// Not "call", that tag is the hands-free stream.
    pub fn name(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::Speech => "speech",
            Self::System => "system",
            Self::Telephony => "telephony",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "media" => Some(Self::Media),
            "speech" => Some(Self::Speech),
            "system" => Some(Self::System),
            "telephony" => Some(Self::Telephony),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChannelEvent {
    Setup { codec: i32, sample_rate: u32, channels: u32 },
    Start,
    Stop,
}

#[derive(Debug)]
pub struct AudioChannel {
    channel: u8,
    session: u32,
    sample_rate: u32,
    channel_count: u32,
}

impl AudioChannel {
    pub fn new(channel: u8) -> Self {
        Self { channel, session: 0, sample_rate: 48000, channel_count: 2 }
    }

    pub fn channel_type(&self) -> AudioChannelType {
        AudioChannelType::of_channel(self.channel)
    }

    pub fn channel_id(&self) -> u8 {
        self.channel
    }

    pub fn format(&self) -> (u32, u32) {
        (self.sample_rate, self.channel_count)
    }

    pub fn handle_message(&mut self, msg_id: u16, payload: &[u8]) -> Option<AudioChannelEvent> {
        let name = self.channel_type().name();
        match msg_id {
            media_msg::START => {
                if let Some(start) = decode_start(payload) {
                    self.session = start.session_id;
                }
                detail!("[AudioChannel:{name}] stream started, session={}", self.session);
                Some(AudioChannelEvent::Start)
            }
            media_msg::STOP => {
                detail!("[AudioChannel:{name}] stream stopped");
                Some(AudioChannelEvent::Stop)
            }
            other => {
                if debug() {
                    println!("[AudioChannel:{name}] unhandled msgId=0x{other:x}");
                }
                None
            }
        }
    }

    pub fn handle_setup_request(
        &mut self,
        codec: i32,
        sample_rate: u32,
        channel_count: u32,
    ) -> AudioChannelEvent {
        if sample_rate != 0 {
            self.sample_rate = sample_rate;
        }
        if channel_count != 0 {
            self.channel_count = channel_count;
        }
        detail!(
            "[AudioChannel:{}] setup codec={codec} {}Hz {}ch",
            self.channel_type().name(),
            self.sample_rate,
            self.channel_count
        );
        AudioChannelEvent::Setup {
            codec,
            sample_rate: self.sample_rate,
            channels: self.channel_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_and_format() {
        let mut a = AudioChannel::new(ch::SPEECH_AUDIO);
        assert_eq!(a.channel_type(), AudioChannelType::Speech);
        assert_eq!(a.format(), (48000, 2));
        assert_eq!(
            a.handle_setup_request(1, 16000, 1),
            AudioChannelEvent::Setup { codec: 1, sample_rate: 16000, channels: 1 }
        );
        assert_eq!(
            a.handle_setup_request(1, 0, 0),
            AudioChannelEvent::Setup { codec: 1, sample_rate: 16000, channels: 1 }
        );
        assert_eq!(
            a.handle_message(media_msg::START, &[0x08, 0x07]),
            Some(AudioChannelEvent::Start)
        );
        assert_eq!(a.session, 7);
        assert_eq!(a.handle_message(media_msg::START, &[]), Some(AudioChannelEvent::Start));
        assert_eq!(a.session, 7);
        assert_eq!(a.handle_message(media_msg::STOP, &[]), Some(AudioChannelEvent::Stop));
        assert_eq!(a.handle_message(media_msg::ACK, &[]), None);
        assert_eq!(AudioChannel::new(42).channel_type(), AudioChannelType::Media);
        for t in [
            AudioChannelType::Media,
            AudioChannelType::Speech,
            AudioChannelType::System,
            AudioChannelType::Telephony,
        ] {
            assert_eq!(AudioChannelType::of_channel(t.channel()), t);
            assert_eq!(AudioChannelType::from_name(t.name()), Some(t));
        }
        assert_eq!(AudioChannelType::from_name("call"), None);
    }
}
