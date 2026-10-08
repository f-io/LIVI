// Android Auto wire-protocol constants, the subset the transport needs.

use livi_aa_proto::{ControlMessageId, MediaMessageId};

pub const TCP_PORT: u16 = 5277;

// Frame flag bits.
pub const FLAG_FIRST: u8 = 0x01;
pub const FLAG_LAST: u8 = 0x02;
pub const FLAG_ENCRYPTED: u8 = 0x08;

// The flag combinations the protocol uses.
pub const FLAGS_PLAINTEXT: u8 = 0x03;
pub const FLAGS_ENC_SIGNAL: u8 = 0x0b;

// Channel ids.
pub const CH_CONTROL: u8 = 0;
pub const CH_VIDEO: u8 = 3;
pub const CH_MEDIA_AUDIO: u8 = 4;
pub const CH_SPEECH_AUDIO: u8 = 5;
pub const CH_SYSTEM_AUDIO: u8 = 6;
pub const CH_TELEPHONY_AUDIO: u8 = 7;
pub const CH_MIC_INPUT: u8 = 9;
pub const CH_CLUSTER_VIDEO: u8 = 19;

pub const CTRL_VERSION_REQUEST: u16 = ControlMessageId::VersionRequest as u16;
pub const CTRL_VERSION_RESPONSE: u16 = ControlMessageId::VersionResponse as u16;
pub const CTRL_ENCAPSULATED_SSL: u16 = ControlMessageId::EncapsulatedSsl as u16;

/// Media with an eight byte timestamp ahead of it.
pub const MEDIA_DATA: u16 = MediaMessageId::Data as u16;
/// Media without a timestamp, the codec configuration.
pub const MEDIA_CODEC_CONFIG: u16 = MediaMessageId::CodecConfig as u16;
pub const MEDIA_START: u16 = MediaMessageId::Start as u16;
pub const MEDIA_ACK: u16 = MediaMessageId::Ack as u16;

// The phone turns features on by the version we ask for, up to its own 6.1.
pub const VERSION_MAJOR: u16 = 6;
pub const VERSION_MINOR: u16 = 1;
pub const VERSION_STATUS_MISMATCH: u16 = 0xffff;

pub fn is_video_channel(ch: u8) -> bool {
    ch == CH_VIDEO || ch == CH_CLUSTER_VIDEO
}

pub fn is_audio_channel(ch: u8) -> bool {
    matches!(ch, CH_MEDIA_AUDIO | CH_SPEECH_AUDIO | CH_SYSTEM_AUDIO | CH_TELEPHONY_AUDIO)
}

pub fn is_media_message(msg_id: u16) -> bool {
    msg_id == MEDIA_DATA || msg_id == MEDIA_CODEC_CONFIG
}
