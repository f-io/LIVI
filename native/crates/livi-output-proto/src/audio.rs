// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    AacLc = 0,
    Opus = 1,
    Pcm = 2,
    PcmLe = 3,
    AacRaw = 4,
    AacEld = 5,
    Alac = 6,
    Alac24 = 7,
}

impl AudioCodec {
    fn from_wire(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::AacLc,
            1 => Self::Opus,
            2 => Self::Pcm,
            3 => Self::PcmLe,
            4 => Self::AacRaw,
            5 => Self::AacEld,
            6 => Self::Alac,
            7 => Self::Alac24,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOpts {
    pub codec: AudioCodec,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub channels: u8,
    pub latency_ms: u32,
    pub realtime: bool,
    pub fed: bool,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicOpts {
    pub pcm: bool,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub frame_ms: u32,
    pub port: u16,
    pub phone: String,
    pub device: String,
}

pub fn aac_raw_key(asc: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    let len = asc.len().min(31);
    key[0] = len as u8;
    key[1..1 + len].copy_from_slice(&asc[..len]);
    key
}

pub fn aac_raw_asc(key: &[u8; 32]) -> &[u8] {
    &key[1..1 + usize::from(key[0]).min(31)]
}

const AUDIO_FIXED: usize = 12;
const MIC_FIXED: usize = 17;

pub fn encode_audio_open(key: &[u8; 32], o: &AudioOpts) -> Vec<u8> {
    let mut out = Vec::with_capacity(AUDIO_FIXED + 32 + o.device.len());
    out.extend_from_slice(&[o.codec as u8, o.payload_type]);
    out.extend_from_slice(&o.clock_rate.to_le_bytes());
    out.push(o.channels);
    out.extend_from_slice(&o.latency_ms.to_le_bytes());
    out.push(u8::from(o.realtime) | (u8::from(o.fed) << 1));
    out.extend_from_slice(key);
    out.extend_from_slice(o.device.as_bytes());
    out
}

pub fn parse_audio_open(rest: &[u8]) -> Option<(AudioOpts, [u8; 32])> {
    let (fixed, rest) = rest.split_first_chunk::<AUDIO_FIXED>()?;
    let (key, device) = rest.split_first_chunk::<32>()?;
    let opts = AudioOpts {
        codec: AudioCodec::from_wire(fixed[0])?,
        payload_type: fixed[1],
        clock_rate: u32::from_le_bytes([fixed[2], fixed[3], fixed[4], fixed[5]]),
        channels: fixed[6],
        latency_ms: u32::from_le_bytes([fixed[7], fixed[8], fixed[9], fixed[10]]),
        realtime: fixed[11] & 1 != 0,
        fed: fixed[11] & 2 != 0,
        device: String::from_utf8(device.to_vec()).ok()?,
    };
    Some((opts, *key))
}

pub fn encode_mic_open(key: &[u8; 32], o: &MicOpts) -> Vec<u8> {
    let phone = &o.phone.as_bytes()[..o.phone.len().min(255)];
    let mut out = Vec::with_capacity(MIC_FIXED + 33 + phone.len() + o.device.len());
    out.extend_from_slice(&[u8::from(o.pcm), o.payload_type]);
    out.extend_from_slice(&o.sample_rate.to_le_bytes());
    out.push(o.channels);
    out.extend_from_slice(&o.bitrate.to_le_bytes());
    out.extend_from_slice(&o.frame_ms.to_le_bytes());
    out.extend_from_slice(&o.port.to_le_bytes());
    out.extend_from_slice(key);
    out.push(phone.len() as u8);
    out.extend_from_slice(phone);
    out.extend_from_slice(o.device.as_bytes());
    out
}

pub fn parse_mic_open(rest: &[u8]) -> Option<(MicOpts, [u8; 32])> {
    let (fixed, rest) = rest.split_first_chunk::<MIC_FIXED>()?;
    let (key, rest) = rest.split_first_chunk::<32>()?;
    let (&phone_len, rest) = rest.split_first()?;
    let (phone, device) = rest.split_at_checked(usize::from(phone_len))?;
    let le32 =
        |at: usize| u32::from_le_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
    let opts = MicOpts {
        pcm: fixed[0] == 1,
        payload_type: fixed[1],
        sample_rate: le32(2),
        channels: fixed[6],
        bitrate: le32(7),
        frame_ms: le32(11),
        port: u16::from_le_bytes([fixed[15], fixed[16]]),
        phone: String::from_utf8_lossy(phone).into_owned(),
        device: String::from_utf8(device.to_vec()).ok()?,
    };
    Some((opts, *key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_open_round_trips() {
        let opts = AudioOpts {
            codec: AudioCodec::AacRaw,
            payload_type: 96,
            clock_rate: 44_100,
            channels: 2,
            latency_ms: 120,
            realtime: true,
            fed: true,
            device: "hw:1".into(),
        };
        let key = aac_raw_key(&[0x12, 0x10]);
        let wire = encode_audio_open(&key, &opts);
        assert_eq!(parse_audio_open(&wire), Some((opts, key)));
        assert_eq!(aac_raw_asc(&key), &[0x12, 0x10]);
    }

    #[test]
    fn mic_open_round_trips() {
        let opts = MicOpts {
            pcm: true,
            payload_type: 100,
            sample_rate: 16_000,
            channels: 1,
            bitrate: 24_000,
            frame_ms: 20,
            port: 7001,
            phone: "phone".into(),
            device: String::new(),
        };
        let wire = encode_mic_open(&[7; 32], &opts);
        assert_eq!(parse_mic_open(&wire), Some((opts, [7; 32])));
    }

    #[test]
    fn short_or_unknown_input_is_refused() {
        let opts = AudioOpts {
            codec: AudioCodec::Opus,
            payload_type: 0,
            clock_rate: 48_000,
            channels: 2,
            latency_ms: 0,
            realtime: false,
            fed: false,
            device: String::new(),
        };
        let mut wire = encode_audio_open(&[0; 32], &opts);
        assert!(parse_audio_open(&wire[..wire.len() - 1]).is_none());
        wire[0] = 99;
        assert!(parse_audio_open(&wire).is_none());
        assert!(parse_mic_open(&[0; 49]).is_none());
    }
}
