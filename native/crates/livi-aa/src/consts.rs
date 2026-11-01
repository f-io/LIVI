// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_aa_proto::consts::{ch, media_msg};

pub const TCP_PORT: u16 = 5277;

pub const VERSION_MAJOR: u16 = 6;
pub const VERSION_MINOR: u16 = 1;
pub const VERSION_STATUS_MISMATCH: u16 = 0xffff;

pub fn is_video_channel(id: u8) -> bool {
    id == ch::VIDEO || id == ch::CLUSTER_VIDEO
}

pub fn is_audio_channel(id: u8) -> bool {
    matches!(
        id,
        ch::MEDIA_AUDIO | ch::SPEECH_AUDIO | ch::SYSTEM_AUDIO | ch::TELEPHONY_AUDIO
    )
}

pub fn is_media_message(msg_id: u16) -> bool {
    msg_id == media_msg::DATA || msg_id == media_msg::CODEC_CONFIG
}
