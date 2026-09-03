// Android Auto wire-protocol constants, the subset the transport needs.

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
pub const CH_MIC_INPUT: u8 = 9;
pub const CH_CLUSTER_VIDEO: u8 = 19;

// Control channel message ids.
pub const CTRL_VERSION_REQUEST: u16 = 0x0001;
pub const CTRL_VERSION_RESPONSE: u16 = 0x0002;
pub const CTRL_SSL_HANDSHAKE: u16 = 0x0003;

// AV channel message ids.
pub const AV_MEDIA_WITH_TIMESTAMP: u16 = 0x0000;
pub const AV_MEDIA_INDICATION: u16 = 0x0001;
pub const AV_SETUP_REQUEST: u16 = 0x8000;
pub const AV_START_INDICATION: u16 = 0x8001;
pub const AV_MEDIA_ACK: u16 = 0x8004;

pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 7;
pub const VERSION_STATUS_MISMATCH: u16 = 0xffff;

pub fn is_video_channel(ch: u8) -> bool {
    ch == CH_VIDEO || ch == CH_CLUSTER_VIDEO
}

pub fn is_audio_channel(ch: u8) -> bool {
    ch == CH_MEDIA_AUDIO || ch == CH_SPEECH_AUDIO || ch == CH_SYSTEM_AUDIO
}

pub fn is_media_message(msg_id: u16) -> bool {
    msg_id == AV_MEDIA_WITH_TIMESTAMP || msg_id == AV_MEDIA_INDICATION
}
