//! The CarPlay microphone stream.
//!
//! Captured audio is encoded, sealed and sent to the phone as RTP. The header
//! is 12 bytes, SSRC 0, and the packet carries the ciphertext, a 16-byte tag
//! and the 8-byte nonce counter, little endian. The timestamp counts samples
//! per packet, not the encoder's own clock.

use livi_crypto_node::seal_impl;

pub const RTP_HEADER_LEN: usize = 12;

/// How the captured samples travel. Wireless CarPlay negotiates Opus, a wired
/// phone takes the samples as they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UplinkCodec {
    Opus,
    Pcm,
}

/// Counts the sequence, timestamp and nonce one stream has sent.
#[derive(Default)]
pub struct Counters {
    pub seq: u16,
    pub timestamp: u32,
    pub nonce: u64,
}

/// The RTP header of the next packet.
pub fn rtp_header(payload_type: u8, seq: u16, timestamp: u32) -> [u8; RTP_HEADER_LEN] {
    let mut header = [0u8; RTP_HEADER_LEN];
    header[0] = 0x80;
    header[1] = payload_type & 0x7f;
    header[2..4].copy_from_slice(&seq.to_be_bytes());
    header[4..8].copy_from_slice(&timestamp.to_be_bytes());
    header
}

/// One packet ready for the phone: header, sealed body, nonce. `samples` is how
/// far the timestamp moves on afterwards.
pub fn seal_packet(
    key: &[u8; 32],
    payload_type: u8,
    counters: &mut Counters,
    body: &[u8],
    samples: u32,
) -> Option<Vec<u8>> {
    let header = rtp_header(payload_type, counters.seq, counters.timestamp);

    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counters.nonce.to_le_bytes());
    let sealed = seal_impl(key, &nonce, body, &header[4..])?;

    let mut packet = Vec::with_capacity(RTP_HEADER_LEN + sealed.len() + 8);
    packet.extend_from_slice(&header);
    packet.extend_from_slice(&sealed);
    packet.extend_from_slice(&counters.nonce.to_le_bytes());

    counters.seq = counters.seq.wrapping_add(1);
    counters.timestamp = counters.timestamp.wrapping_add(samples);
    counters.nonce += 1;
    Some(packet)
}

/// Samples travel big endian, so a PCM body is byte-swapped before sealing.
pub fn to_wire_pcm(pcm_le: &[u8]) -> Vec<u8> {
    let mut out = pcm_le.to_vec();
    for pair in out.as_chunks_mut::<2>().0 {
        pair.swap(0, 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_crypto_node::open_impl;

    const KEY: [u8; 32] = [3u8; 32];

    #[test]
    fn the_header_carries_the_payload_type_sequence_and_timestamp() {
        let h = rtp_header(97, 0x1234, 0xdeadbeef);

        assert_eq!(h[0], 0x80);
        assert_eq!(h[1], 97);
        assert_eq!(&h[2..4], &0x1234u16.to_be_bytes());
        assert_eq!(&h[4..8], &0xdeadbeefu32.to_be_bytes());
        assert_eq!(&h[8..], &[0, 0, 0, 0]);
    }

    #[test]
    fn a_packet_opens_again_with_the_header_as_associated_data() {
        let mut c = Counters::default();
        let packet = seal_packet(&KEY, 97, &mut c, b"hello", 480).unwrap();

        let end = packet.len();
        let nonce8 = &packet[end - 8..];
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(nonce8);
        let body = &packet[RTP_HEADER_LEN..end - 8];
        let plain = open_impl(&KEY, &nonce, body, &packet[4..RTP_HEADER_LEN]).unwrap();

        assert_eq!(plain, b"hello");
    }

    #[test]
    fn the_counters_move_on_by_one_packet_and_its_samples() {
        let mut c = Counters::default();

        seal_packet(&KEY, 97, &mut c, b"a", 480).unwrap();
        seal_packet(&KEY, 97, &mut c, b"b", 480).unwrap();

        assert_eq!(c.seq, 2);
        assert_eq!(c.timestamp, 960);
        assert_eq!(c.nonce, 2);
    }

    #[test]
    fn the_sequence_wraps_at_sixteen_bits() {
        let mut c = Counters { seq: 0xffff, timestamp: 0, nonce: 0 };

        seal_packet(&KEY, 97, &mut c, b"a", 0).unwrap();

        assert_eq!(c.seq, 0);
    }

    #[test]
    fn every_second_byte_leads_on_the_wire() {
        assert_eq!(to_wire_pcm(&[0x01, 0x02, 0x03, 0x04]), vec![0x02, 0x01, 0x04, 0x03]);
    }

    #[test]
    fn an_odd_trailing_byte_is_left_alone() {
        assert_eq!(to_wire_pcm(&[0x01, 0x02, 0x03]), vec![0x02, 0x01, 0x03]);
    }
}
