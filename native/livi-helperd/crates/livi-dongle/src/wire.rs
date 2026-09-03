// CarlinKit framing: a 16 byte header of magic, payload length, message type and
// the type's complement, then the payload.

pub const MAGIC: u32 = 0x55aa_55aa;
pub const HEADER_LEN: usize = 16;
/// A longer payload is a framing error, not a message.
pub const MAX_PAYLOAD: usize = 16 << 20;

pub const OPEN: u32 = 0x01;
pub const UNPLUGGED: u32 = 0x04;
pub const VIDEO: u32 = 0x06;
pub const AUDIO: u32 = 0x07;
pub const CLUSTER_VIDEO: u32 = 0x2c;
pub const HEARTBEAT: u32 = 0xaa;

/// Width, height, flags, length and one unknown word precede the H.264 data.
pub const VIDEO_HEAD: usize = 20;
/// Decode type, volume and audio type precede the samples.
pub const AUDIO_HEAD: usize = 12;

pub fn header(kind: u32, len: usize) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(&MAGIC.to_le_bytes());
    h[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    h[8..12].copy_from_slice(&kind.to_le_bytes());
    h[12..].copy_from_slice(&(!kind).to_le_bytes());
    h
}

pub fn message(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header(kind, payload.len()));
    out.extend_from_slice(payload);
    out
}

/// The message type and payload length of a header.
pub fn parse_header(h: &[u8; HEADER_LEN]) -> Result<(u32, usize), String> {
    let word = |i: usize| u32::from_le_bytes(h[i..i + 4].try_into().unwrap());
    if word(0) != MAGIC {
        return Err(format!("bad magic {:#010x}", word(0)));
    }
    let (len, kind, check) = (word(4), word(8), word(12));
    if check != !kind {
        return Err(format!("type {kind:#x} fails its check {check:#x}"));
    }
    if len as usize > MAX_PAYLOAD {
        return Err(format!("payload of {len} bytes"));
    }
    Ok((kind, len as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips_through_its_header() {
        let wire = message(VIDEO, &[1, 2, 3]);
        assert_eq!(wire.len(), HEADER_LEN + 3);
        let head: [u8; HEADER_LEN] = wire[..HEADER_LEN].try_into().unwrap();
        assert_eq!(parse_header(&head), Ok((VIDEO, 3)));
        assert_eq!(&wire[HEADER_LEN..], &[1, 2, 3]);
    }

    #[test]
    fn the_header_matches_the_main_process_layout() {
        let h = header(HEARTBEAT, 0);
        assert_eq!(&h[..4], &[0xaa, 0x55, 0xaa, 0x55]);
        assert_eq!(&h[4..8], &[0, 0, 0, 0]);
        assert_eq!(&h[8..12], &[0xaa, 0, 0, 0]);
        assert_eq!(&h[12..], &[0x55, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn a_wrong_magic_check_or_size_is_refused() {
        let mut h = header(AUDIO, 8);
        h[0] = 0;
        assert!(parse_header(&h).unwrap_err().contains("magic"));
        let mut h = header(AUDIO, 8);
        h[12] ^= 1;
        assert!(parse_header(&h).unwrap_err().contains("check"));
        let h = header(AUDIO, MAX_PAYLOAD + 1);
        assert!(parse_header(&h).unwrap_err().contains("bytes"));
    }
}
