// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const VIDEO: u64 = 1 << 0;
pub const PHOTO: u64 = 1 << 1;
pub const VIDEO_FAIRPLAY: u64 = 1 << 2;
pub const VIDEO_HLS: u64 = 1 << 4;
pub const SLIDESHOW: u64 = 1 << 5;
pub const UNKNOWN_6: u64 = 1 << 6;
pub const SCREEN: u64 = 1 << 7;
pub const SCREEN_ROTATE: u64 = 1 << 8;
pub const AUDIO: u64 = 1 << 9;
pub const UNKNOWN_10: u64 = 1 << 10;
pub const AUDIO_REDUNDANT: u64 = 1 << 11;
pub const PHOTO_CACHING: u64 = 1 << 13;
pub const META_ARTWORK: u64 = 1 << 15;
pub const META_PROGRESS: u64 = 1 << 16;
pub const META_TEXT: u64 = 1 << 17;
pub const AUDIO_FORMAT_1: u64 = 1 << 18;
pub const AUDIO_FORMAT_2: u64 = 1 << 19;
pub const AUDIO_FORMAT_3: u64 = 1 << 20;
pub const AUDIO_FORMAT_4: u64 = 1 << 21;
pub const AUDIO_UNENCRYPTED: u64 = 1 << 22;
pub const ITUNES4W_ENCRYPTION: u64 = 1 << 25;
pub const AUDIO_AES_MFI: u64 = 1 << 26;
pub const UNKNOWN_28: u64 = 1 << 28;
pub const RAOP: u64 = 1 << 30;
pub const CARPLAY: u64 = 1 << 32;
pub const VIDEO_PLAY_QUEUE: u64 = 1 << 33;
pub const FROM_CLOUD: u64 = 1 << 34;
pub const TLS_PSK: u64 = 1 << 35;
pub const UNKNOWN_36: u64 = 1 << 36;
pub const CARPLAY_CONTROL: u64 = 1 << 37;
pub const COREUTILS_PAIRING: u64 = 1 << 38;
pub const BUFFERED_AUDIO: u64 = 1 << 40;
pub const PTP: u64 = 1 << 41;
pub const HOMEKIT_PAIRING: u64 = 1 << 46;
pub const PEER_MANAGEMENT: u64 = 1 << 47;
pub const TRANSIENT_PAIRING: u64 = 1 << 48;
pub const META_PLIST: u64 = 1 << 50;
pub const HANGDOG_REMOTE: u64 = 1 << 58;
pub const AUDIO_MEDIA_DATA_CONTROL: u64 = 1 << 60;

pub const CARPLAY_FEATURES: u64 = SCREEN
    | SCREEN_ROTATE
    | AUDIO
    | AUDIO_FORMAT_1
    | AUDIO_FORMAT_3
    | AUDIO_UNENCRYPTED
    | AUDIO_AES_MFI
    | RAOP
    | CARPLAY
    | CARPLAY_CONTROL
    | COREUTILS_PAIRING;
pub const CARPLAY_AUDIO_FEATURES: u64 = AUDIO
    | AUDIO_REDUNDANT
    | AUDIO_FORMAT_1
    | AUDIO_FORMAT_3
    | AUDIO_UNENCRYPTED
    | AUDIO_AES_MFI
    | BUFFERED_AUDIO;

pub fn txt(features: u64) -> String {
    format!("{:#X},{:#X}", features as u32, features >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carplay_announces_the_same_bits_in_bonjour_and_info() {
        assert_eq!(txt(CARPLAY_FEATURES), "0x44540380,0x61");
        assert_eq!(CARPLAY_AUDIO_FEATURES, 0x100_0454_0a00);
    }
}
