// NTB16 framing (CDC NCM): ethernet frames packed into NTB blocks.

pub const NTH16_SIG: u32 = 0x484D_434E;
pub const NDP16_SIG: u32 = 0x304D_434E;

fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Extracts the ethernet frames carried in one NTB block.
pub fn parse_ntb(ntb: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    if ntb.len() < 12 || u32le(ntb, 0) != NTH16_SIG {
        return frames;
    }
    let mut ndp_idx = u16le(ntb, 10) as usize;
    while ndp_idx != 0 && ndp_idx + 12 <= ntb.len() {
        let nsig = u32le(ntb, ndp_idx);
        if nsig & 0x00FF_FFFF != NDP16_SIG & 0x00FF_FFFF {
            break;
        }
        let nlen = u16le(ntb, ndp_idx + 4) as usize;
        let next_ndp = u16le(ntb, ndp_idx + 6) as usize;
        let mut off = ndp_idx + 8;
        let end = (ndp_idx + nlen).min(ntb.len());
        while off + 4 <= end {
            let d_idx = u16le(ntb, off) as usize;
            let d_len = u16le(ntb, off + 2) as usize;
            if d_idx == 0 || d_len == 0 {
                break;
            }
            if d_idx + d_len <= ntb.len() {
                frames.push(ntb[d_idx..d_idx + d_len].to_vec());
            }
            off += 4;
        }
        ndp_idx = next_ndp;
    }
    frames
}

/// Wraps one ethernet frame in an NTB block. A block that lands exactly on a USB packet
/// boundary gets a pad byte so the transfer is not read as a short packet.
pub fn build_ntb(frame: &[u8], seq: u16) -> Vec<u8> {
    const D_IDX: u16 = 28;
    let blen = D_IDX as usize + frame.len();

    let mut ntb = Vec::with_capacity(blen + 1);
    ntb.extend_from_slice(&NTH16_SIG.to_le_bytes());
    ntb.extend_from_slice(&12u16.to_le_bytes());
    ntb.extend_from_slice(&seq.to_le_bytes());
    ntb.extend_from_slice(&(blen as u16).to_le_bytes());
    ntb.extend_from_slice(&12u16.to_le_bytes());

    ntb.extend_from_slice(&NDP16_SIG.to_le_bytes());
    ntb.extend_from_slice(&16u16.to_le_bytes());
    ntb.extend_from_slice(&0u16.to_le_bytes());
    ntb.extend_from_slice(&D_IDX.to_le_bytes());
    ntb.extend_from_slice(&(frame.len() as u16).to_le_bytes());
    ntb.extend_from_slice(&0u16.to_le_bytes());
    ntb.extend_from_slice(&0u16.to_le_bytes());

    ntb.extend_from_slice(frame);
    if ntb.len() % 512 == 0 {
        ntb.push(0);
    }
    ntb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_frame() {
        let frame = vec![0xAA; 64];
        let ntb = build_ntb(&frame, 7);
        assert_eq!(parse_ntb(&ntb), vec![frame]);
    }

    #[test]
    fn pads_on_packet_boundary() {
        let frame = vec![0x11; 512 - 28];
        let ntb = build_ntb(&frame, 1);
        assert_eq!(ntb.len(), 513, "must not end on a 512-byte boundary");
        assert_eq!(parse_ntb(&ntb), vec![frame]);
    }

    #[test]
    fn parses_multiple_datagrams() {
        // Two datagrams in one NDP table.
        let a = vec![1u8; 10];
        let b = vec![2u8; 20];
        let d_a = 32usize;
        let d_b = d_a + a.len();
        let blen = d_b + b.len();
        let mut ntb = Vec::new();
        ntb.extend_from_slice(&NTH16_SIG.to_le_bytes());
        ntb.extend_from_slice(&12u16.to_le_bytes());
        ntb.extend_from_slice(&0u16.to_le_bytes());
        ntb.extend_from_slice(&(blen as u16).to_le_bytes());
        ntb.extend_from_slice(&12u16.to_le_bytes());
        ntb.extend_from_slice(&NDP16_SIG.to_le_bytes());
        ntb.extend_from_slice(&20u16.to_le_bytes());
        ntb.extend_from_slice(&0u16.to_le_bytes());
        ntb.extend_from_slice(&(d_a as u16).to_le_bytes());
        ntb.extend_from_slice(&(a.len() as u16).to_le_bytes());
        ntb.extend_from_slice(&(d_b as u16).to_le_bytes());
        ntb.extend_from_slice(&(b.len() as u16).to_le_bytes());
        ntb.extend_from_slice(&0u16.to_le_bytes());
        ntb.extend_from_slice(&0u16.to_le_bytes());
        ntb.resize(d_a, 0);
        ntb.extend_from_slice(&a);
        ntb.extend_from_slice(&b);
        assert_eq!(parse_ntb(&ntb), vec![a, b]);
    }

    #[test]
    fn rejects_foreign_block() {
        assert!(parse_ntb(&[0u8; 32]).is_empty());
    }
}
