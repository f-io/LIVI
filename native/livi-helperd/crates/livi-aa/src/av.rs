// The AV channel messages the transport handles itself: media data, the start
// indication it takes the session id from, and the ack older protocols need.

use livi_aa_proto::{MediaAckNotification, MediaStartNotification};
use prost::Message;

use crate::consts::{MEDIA_DATA, is_video_channel};

/// The media bytes and their timestamp. Only `MEDIA_DATA` carries one, as eight
/// big-endian bytes ahead of the data.
pub fn media(msg_id: u16, payload: &[u8]) -> (Option<u64>, &[u8]) {
    if msg_id == MEDIA_DATA && payload.len() >= 8 {
        let ts = u64::from_be_bytes(payload[..8].try_into().unwrap());
        return (Some(ts), &payload[8..]);
    }
    (None, payload)
}

pub fn start_session_id(payload: &[u8]) -> Option<u32> {
    MediaStartNotification::decode(payload).ok().map(|s| s.session_id as u32)
}

/// One ack per media message.
pub fn ack(session_id: u32) -> Vec<u8> {
    MediaAckNotification {
        session_id: session_id as i32,
        acked_frame_count: Some(1),
        receive_times_ns: Vec::new(),
    }
    .encode_to_vec()
}

/// From protocol 5.0 the phone streams audio without waiting for acks, from 6.0 video too, and
/// takes every ack it still gets for a message of unknown type.
pub fn wants_ack(protocol: (u16, u16), ch: u8) -> bool {
    protocol < if is_video_channel(ch) { (6, 0) } else { (5, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::MEDIA_CODEC_CONFIG;

    #[test]
    fn timestamped_media_is_split() {
        let mut payload = 12345u64.to_be_bytes().to_vec();
        payload.extend_from_slice(b"nal");
        assert_eq!(media(MEDIA_DATA, &payload), (Some(12345), &b"nal"[..]));
    }

    #[test]
    fn plain_media_has_no_timestamp() {
        assert_eq!(media(MEDIA_CODEC_CONFIG, b"sps"), (None, &b"sps"[..]));
    }

    #[test]
    fn session_id_comes_out_of_start() {
        let start = MediaStartNotification { session_id: 300, ..Default::default() };
        assert_eq!(start_session_id(&start.encode_to_vec()), Some(300));
    }

    #[test]
    fn ack_matches_the_reference_shape() {
        assert_eq!(ack(1), vec![0x08, 0x01, 0x10, 0x01]);
    }

    #[test]
    fn acks_end_with_the_protocol() {
        use crate::consts::{CH_CLUSTER_VIDEO, CH_MEDIA_AUDIO, CH_VIDEO};
        assert!(wants_ack((1, 7), CH_VIDEO));
        assert!(wants_ack((5, 1), CH_CLUSTER_VIDEO));
        assert!(!wants_ack((6, 0), CH_VIDEO));
        assert!(wants_ack((4, 9), CH_MEDIA_AUDIO));
        assert!(!wants_ack((5, 0), CH_MEDIA_AUDIO));
        assert!(!wants_ack((6, 1), CH_MEDIA_AUDIO));
    }
}
