// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_aa_proto::{MediaAckNotification, MediaStartNotification};
use prost::Message;

use livi_aa_proto::consts::media_msg;

use crate::consts::is_video_channel;

pub fn media(msg_id: u16, payload: &[u8]) -> (Option<u64>, &[u8]) {
    if msg_id == media_msg::DATA && payload.len() >= 8 {
        let ts = u64::from_be_bytes(payload[..8].try_into().unwrap());
        return (Some(ts), &payload[8..]);
    }
    (None, payload)
}

pub fn start_session_id(payload: &[u8]) -> Option<u32> {
    MediaStartNotification::decode(payload)
        .ok()
        .map(|s| s.session_id as u32)
}

pub fn ack(session_id: u32) -> Vec<u8> {
    MediaAckNotification {
        session_id: session_id as i32,
        acked_frame_count: Some(1),
        receive_times_ns: Vec::new(),
    }
    .encode_to_vec()
}

pub fn wants_ack(protocol: (u16, u16), ch: u8) -> bool {
    protocol < if is_video_channel(ch) { (6, 0) } else { (5, 0) }
}

#[cfg(test)]
mod tests {
    use livi_aa_proto::consts::ch;

    use super::*;

    #[test]
    fn timestamped_media_is_split() {
        let mut payload = 12345u64.to_be_bytes().to_vec();
        payload.extend_from_slice(b"nal");
        assert_eq!(media(media_msg::DATA, &payload), (Some(12345), &b"nal"[..]));
    }

    #[test]
    fn plain_media_has_no_timestamp() {
        assert_eq!(media(media_msg::CODEC_CONFIG, b"sps"), (None, &b"sps"[..]));
    }

    #[test]
    fn session_id_comes_out_of_start() {
        let start = MediaStartNotification {
            session_id: 300,
            ..Default::default()
        };
        assert_eq!(start_session_id(&start.encode_to_vec()), Some(300));
    }

    #[test]
    fn ack_matches_the_reference_shape() {
        assert_eq!(ack(1), vec![0x08, 0x01, 0x10, 0x01]);
    }

    #[test]
    fn acks_end_with_the_protocol() {
        assert!(wants_ack((1, 7), ch::VIDEO));
        assert!(wants_ack((5, 1), ch::CLUSTER_VIDEO));
        assert!(!wants_ack((6, 0), ch::VIDEO));
        assert!(wants_ack((4, 9), ch::MEDIA_AUDIO));
        assert!(!wants_ack((5, 0), ch::MEDIA_AUDIO));
        assert!(!wants_ack((6, 1), ch::MEDIA_AUDIO));
    }
}
