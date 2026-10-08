/// Byte 1 of the frame header.
pub mod frame_flags {
    pub const PLAINTEXT: u8 = 0x03;
    pub const ENC_SIGNAL: u8 = 0x0b;
    pub const ENC_CONTROL: u8 = 0x0f;
    pub const ENC_FIRST_FRAG: u8 = 0x08;
    pub const ENC_CONT_FRAG: u8 = 0x0a;
    pub const ENCRYPTED: u8 = 0x08;
}

// Message ids end up in `match` arms, so each channel's ids are constants taken from the
// protocol's id enums.

pub mod ctrl_msg {
    use livi_aa_proto::ControlMessageId as Id;

    pub const VERSION_REQUEST: u16 = Id::VersionRequest as u16;
    pub const VERSION_RESPONSE: u16 = Id::VersionResponse as u16;
    pub const ENCAPSULATED_SSL: u16 = Id::EncapsulatedSsl as u16;
    pub const AUTH_COMPLETE: u16 = Id::AuthComplete as u16;
    /// Phone to head unit, despite the name.
    pub const SERVICE_DISCOVERY_REQUEST: u16 = Id::ServiceDiscoveryRequest as u16;
    pub const SERVICE_DISCOVERY_RESPONSE: u16 = Id::ServiceDiscoveryResponse as u16;
    /// The phone opens every channel.
    pub const CHANNEL_OPEN_REQUEST: u16 = Id::ChannelOpenRequest as u16;
    pub const CHANNEL_OPEN_RESPONSE: u16 = Id::ChannelOpenResponse as u16;
    pub const CHANNEL_CLOSE_NOTIFICATION: u16 = Id::ChannelCloseNotification as u16;
    pub const PING_REQUEST: u16 = Id::PingRequest as u16;
    pub const PING_RESPONSE: u16 = Id::PingResponse as u16;
    pub const NAV_FOCUS_REQUEST: u16 = Id::NavFocusRequest as u16;
    pub const NAV_FOCUS_NOTIFICATION: u16 = Id::NavFocusNotification as u16;
    pub const BYEBYE_REQUEST: u16 = Id::ByebyeRequest as u16;
    pub const BYEBYE_RESPONSE: u16 = Id::ByebyeResponse as u16;
    /// Phone to head unit, 1 start and 2 end.
    pub const VOICE_SESSION_NOTIFICATION: u16 = Id::VoiceSessionNotification as u16;
    pub const AUDIO_FOCUS_REQUEST: u16 = Id::AudioFocusRequest as u16;
    pub const AUDIO_FOCUS_NOTIFICATION: u16 = Id::AudioFocusNotification as u16;
    pub const BATTERY_STATUS_NOTIFICATION: u16 = Id::BatteryStatusNotification as u16;
}

/// The audio, video and microphone channels.
pub mod media_msg {
    use livi_aa_proto::MediaMessageId as Id;

    /// Media with an eight byte timestamp ahead of it.
    pub const DATA: u16 = Id::Data as u16;
    /// Media without a timestamp, the codec configuration.
    pub const CODEC_CONFIG: u16 = Id::CodecConfig as u16;
    pub const SETUP: u16 = Id::Setup as u16;
    pub const START: u16 = Id::Start as u16;
    pub const STOP: u16 = Id::Stop as u16;
    pub const CONFIG: u16 = Id::Config as u16;
    pub const ACK: u16 = Id::Ack as u16;
    pub const MICROPHONE_REQUEST: u16 = Id::MicrophoneRequest as u16;
    pub const MICROPHONE_RESPONSE: u16 = Id::MicrophoneResponse as u16;
    pub const VIDEO_FOCUS_REQUEST: u16 = Id::VideoFocusRequest as u16;
    pub const VIDEO_FOCUS_NOTIFICATION: u16 = Id::VideoFocusNotification as u16;
}

pub mod sensor_msg {
    use livi_aa_proto::SensorMessageId as Id;

    pub const REQUEST: u16 = Id::Request as u16;
    pub const RESPONSE: u16 = Id::Response as u16;
    pub const BATCH: u16 = Id::Batch as u16;
}

pub mod input_msg {
    use livi_aa_proto::InputSourceMessageId as Id;

    pub const INPUT_REPORT: u16 = Id::InputReport as u16;
    /// Phone to head unit.
    pub const KEY_BINDING_REQUEST: u16 = Id::KeyBindingRequest as u16;
    pub const KEY_BINDING_RESPONSE: u16 = Id::KeyBindingResponse as u16;
    pub const INPUT_FEEDBACK: u16 = Id::InputFeedback as u16;
}

pub mod nav_msg {
    use livi_aa_proto::NavigationStatusMessageId as Id;

    pub const START: u16 = Id::Start as u16;
    pub const STOP: u16 = Id::Stop as u16;
    pub const STATUS: u16 = Id::Status as u16;
    pub const NEXT_TURN: u16 = Id::NextTurn as u16;
    pub const NEXT_TURN_DISTANCE: u16 = Id::NextTurnDistance as u16;
    pub const STATE: u16 = Id::State as u16;
    pub const CURRENT_POSITION: u16 = Id::CurrentPosition as u16;
}

pub mod playback_msg {
    use livi_aa_proto::MediaPlaybackStatusMessageId as Id;

    pub const STATUS: u16 = Id::Status as u16;
    /// Head unit to phone.
    pub const INPUT: u16 = Id::Input as u16;
    pub const METADATA: u16 = Id::Metadata as u16;
}

pub mod phone_msg {
    use livi_aa_proto::PhoneStatusMessageId as Id;

    pub const STATUS: u16 = Id::Status as u16;
}

pub mod bluetooth_msg {
    use livi_aa_proto::BluetoothMessageId as Id;

    pub const PAIRING_REQUEST: u16 = Id::PairingRequest as u16;
    pub const PAIRING_RESPONSE: u16 = Id::PairingResponse as u16;
}

pub mod wifi_msg {
    use livi_aa_proto::WifiProjectionMessageId as Id;

    pub const CREDENTIALS_REQUEST: u16 = Id::CredentialsRequest as u16;
    pub const CREDENTIALS_RESPONSE: u16 = Id::CredentialsResponse as u16;
}

pub mod ch {
    pub const CONTROL: u8 = 0;
    pub const SENSOR: u8 = 1;
    pub const VIDEO: u8 = 3;
    pub const MEDIA_AUDIO: u8 = 4;
    pub const SPEECH_AUDIO: u8 = 5;
    pub const SYSTEM_AUDIO: u8 = 6;
    pub const TELEPHONY_AUDIO: u8 = 7;
    pub const INPUT: u8 = 8;
    pub const MIC_INPUT: u8 = 9;
    pub const BLUETOOTH: u8 = 10;
    pub const NAVIGATION: u8 = 12;
    pub const MEDIA_INFO: u8 = 13;
    pub const PHONE_STATUS: u8 = 14;
    pub const WIFI: u8 = 18;
    pub const CLUSTER_VIDEO: u8 = 19;
    /// The secondary display's input, not interactive.
    pub const CLUSTER_INPUT: u8 = 20;
}
