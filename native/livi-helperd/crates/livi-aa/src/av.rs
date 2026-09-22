// The AV channel messages the transport handles itself: media data, the start
// indication it takes the session id from, and the ack every media message needs.

use crate::consts::AV_MEDIA_WITH_TIMESTAMP;
use crate::proto::{pb_varint, varint_field};

/// The media bytes and their timestamp. Only `AV_MEDIA_WITH_TIMESTAMP` carries
/// one, as eight big-endian bytes ahead of the data.
pub fn media(msg_id: u16, payload: &[u8]) -> (Option<u64>, &[u8]) {
    if msg_id == AV_MEDIA_WITH_TIMESTAMP && payload.len() >= 8 {
        let ts = u64::from_be_bytes(payload[..8].try_into().unwrap());
        return (Some(ts), &payload[8..]);
    }
    (None, payload)
}

/// `Start { session_id = 1 }`.
pub fn start_session_id(payload: &[u8]) -> Option<u32> {
    varint_field(payload, 1).map(|v| v as u32)
}

/// `SetupRequest { media_codec_type = 1 }`.
pub fn setup_codec(payload: &[u8]) -> Option<u64> {
    varint_field(payload, 1)
}

/// `Ack { session_id = 1, ack = 2 }`, one ack per media message.
pub fn ack(session_id: u32) -> Vec<u8> {
    let mut out = pb_varint(1, session_id as u64);
    out.extend(pb_varint(2, 1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::AV_MEDIA_INDICATION;

    #[test]
    fn timestamped_media_is_split() {
        let mut payload = 12345u64.to_be_bytes().to_vec();
        payload.extend_from_slice(b"nal");
        assert_eq!(
            media(AV_MEDIA_WITH_TIMESTAMP, &payload),
            (Some(12345), &b"nal"[..])
        );
    }

    #[test]
    fn plain_media_has_no_timestamp() {
        assert_eq!(media(AV_MEDIA_INDICATION, b"sps"), (None, &b"sps"[..]));
    }

    #[test]
    fn session_id_comes_out_of_start() {
        let mut body = pb_varint(1, 300);
        body.extend(pb_varint(2, 0));
        assert_eq!(start_session_id(&body), Some(300));
    }

    #[test]
    fn ack_matches_the_reference_shape() {
        assert_eq!(ack(1), vec![0x08, 0x01, 0x10, 0x01]);
    }
}
