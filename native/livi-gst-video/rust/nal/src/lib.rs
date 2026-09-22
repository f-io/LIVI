//! Codec detection and NAL classification for the CarPlay screen stream.
//!
//! `CpCodec`'s value travels in the configuration record the host forwards, so
//! it keeps its order.

/// The codec a screen stream carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpCodec {
    H264 = 0,
    H265 = 1,
}

/// What a NAL is, ordered so the greater value wins.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CpNalKind {
    Delta = 0,
    Params = 1,
    Keyframe = 2,
}

/// True when the buffer opens like an AVCDecoderConfigurationRecord: at least
/// one SPS announced, its length inside the buffer, and an SPS NAL behind it.
fn looks_like_avcc(a: &[u8]) -> bool {
    if a.len() < 9 {
        return false;
    }
    if a[5] & 0x1f < 1 {
        return false;
    }
    let sps_len = ((a[6] as usize) << 8) | a[7] as usize;
    if 8 + sps_len > a.len() {
        return false;
    }
    a[8] & 0x1f == 7
}

/// The codec the payload carries, and where its codec data starts. A `hvcC` or
/// `avcC` marker names the codec outright. Without one the shape of the buffer
/// decides and the whole payload is the codec data.
pub fn detect_codec(payload: &[u8]) -> (CpCodec, usize) {
    let mut i = 4;
    while i + 4 <= payload.len() {
        match &payload[i..i + 4] {
            b"hvcC" => return (CpCodec::H265, i + 4),
            b"avcC" => return (CpCodec::H264, i + 4),
            _ => i += 1,
        }
    }
    if looks_like_avcc(payload) {
        (CpCodec::H264, 0)
    } else {
        (CpCodec::H265, 0)
    }
}

fn classify_byte(header_byte: u8, codec: CpCodec) -> CpNalKind {
    if codec == CpCodec::H265 {
        let t = (header_byte >> 1) & 0x3f;
        return match t {
            16..=23 => CpNalKind::Keyframe,
            32..=34 => CpNalKind::Params,
            _ => CpNalKind::Delta,
        };
    }
    match header_byte & 0x1f {
        5 => CpNalKind::Keyframe,
        7 | 8 => CpNalKind::Params,
        _ => CpNalKind::Delta,
    }
}

/// Reports the most significant kind among the frame's NALs. A frame opening
/// with a four byte start code is Annex B (Android Auto), anything else is
/// length prefixed (CarPlay).
pub fn classify_nal(frame: &[u8], codec: CpCodec) -> CpNalKind {
    if frame.starts_with(&[0, 0, 0, 1]) {
        classify_annex_b(frame, codec)
    } else {
        classify_length_prefixed(frame, codec)
    }
}

/// Every `00 00 01` is a start code, emulation prevention keeps the pattern
/// out of NAL payloads. The header byte follows it.
fn classify_annex_b(frame: &[u8], codec: CpCodec) -> CpNalKind {
    let mut best = CpNalKind::Delta;
    let mut i = 0usize;
    while i + 3 < frame.len() {
        if frame[i] == 0 && frame[i + 1] == 0 && frame[i + 2] == 1 {
            let kind = classify_byte(frame[i + 3], codec);
            if kind > best {
                best = kind;
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    best
}

/// A zero length or one reaching past the frame ends the walk.
fn classify_length_prefixed(frame: &[u8], codec: CpCodec) -> CpNalKind {
    let mut best = CpNalKind::Delta;
    let mut off = 0usize;
    while off + 4 <= frame.len() {
        let nlen = u32::from_be_bytes([frame[off], frame[off + 1], frame[off + 2], frame[off + 3]])
            as usize;
        off += 4;
        if nlen == 0 || off + nlen > frame.len() {
            break;
        }
        let kind = classify_byte(frame[off], codec);
        if kind > best {
            best = kind;
        }
        off += nlen;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h264_nal(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 1) as u32).to_be_bytes().to_vec();
        v.push(kind);
        v.extend_from_slice(body);
        v
    }

    fn h265_nal(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 1) as u32).to_be_bytes().to_vec();
        v.push(kind << 1);
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn a_marker_names_the_codec_and_the_data_behind_it() {
        let mut p = vec![0u8; 4];
        p.extend_from_slice(b"hvcC");
        p.extend_from_slice(&[1, 2, 3]);
        assert_eq!(detect_codec(&p), (CpCodec::H265, 8));

        let mut p = vec![0u8; 4];
        p.extend_from_slice(b"avcC");
        p.extend_from_slice(&[9]);
        assert_eq!(detect_codec(&p), (CpCodec::H264, 8));
    }

    #[test]
    fn the_first_four_bytes_are_never_searched() {
        let mut p = b"avcC".to_vec();
        p.extend_from_slice(&[0; 8]);
        assert_eq!(detect_codec(&p).0, CpCodec::H265);
    }

    #[test]
    fn without_a_marker_the_shape_decides() {
        // avcC-shaped: one SPS announced, its length fits, an SPS NAL behind it
        let avcc = [0u8, 0, 0, 0, 0, 0xe1, 0x00, 0x01, 0x67, 0x00];
        assert_eq!(detect_codec(&avcc), (CpCodec::H264, 0));

        let plain = [0u8; 32];
        assert_eq!(detect_codec(&plain), (CpCodec::H265, 0));
    }

    #[test]
    fn a_buffer_that_cannot_be_avcc_falls_to_h265() {
        assert_eq!(detect_codec(&[0u8; 8]).0, CpCodec::H265); // too short
        let no_sps = [0u8, 0, 0, 0, 0, 0xe0, 0x00, 0x01, 0x67, 0x00];
        assert_eq!(detect_codec(&no_sps).0, CpCodec::H265); // announces none
        let overlong = [0u8, 0, 0, 0, 0, 0xe1, 0xff, 0xff, 0x67, 0x00];
        assert_eq!(detect_codec(&overlong).0, CpCodec::H265); // length past the end
        let wrong_nal = [0u8, 0, 0, 0, 0, 0xe1, 0x00, 0x01, 0x41, 0x00];
        assert_eq!(detect_codec(&wrong_nal).0, CpCodec::H265); // not an SPS
    }

    fn annex_b(nals: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for n in nals {
            v.extend_from_slice(&[0, 0, 0, 1]);
            v.extend_from_slice(n);
        }
        v
    }

    #[test]
    fn annex_b_keyframes_hide_behind_their_parameter_sets() {
        let au = annex_b(&[&[0x67, 0, 0], &[0x68, 0], &[0x65, 0, 0, 0]]);
        assert_eq!(classify_nal(&au, CpCodec::H264), CpNalKind::Keyframe);
        let delta = annex_b(&[&[0x09, 0xf0], &[0x41, 0, 0]]);
        assert_eq!(classify_nal(&delta, CpCodec::H264), CpNalKind::Delta);
        let params = annex_b(&[&[0x67, 0, 0], &[0x68, 0]]);
        assert_eq!(classify_nal(&params, CpCodec::H264), CpNalKind::Params);
    }

    #[test]
    fn annex_b_accepts_three_byte_start_codes_inside_the_frame() {
        let mut au = annex_b(&[&[0x67, 0, 0]]);
        au.extend_from_slice(&[0, 0, 1, 0x65, 0, 0]);
        assert_eq!(classify_nal(&au, CpCodec::H264), CpNalKind::Keyframe);
    }

    #[test]
    fn annex_b_h265() {
        let au = annex_b(&[&[32 << 1, 1], &[19 << 1, 1, 0]]);
        assert_eq!(classify_nal(&au, CpCodec::H265), CpNalKind::Keyframe);
        let delta = annex_b(&[&[1 << 1, 1, 0]]);
        assert_eq!(classify_nal(&delta, CpCodec::H265), CpNalKind::Delta);
    }

    #[test]
    fn a_length_that_reads_like_a_short_start_code_stays_length_prefixed() {
        let frame = h264_nal(1, &[0; 260]);
        assert_eq!(frame[..4], [0, 0, 1, 5]);
        assert_eq!(classify_nal(&frame, CpCodec::H264), CpNalKind::Delta);
    }

    #[test]
    fn h264_kinds() {
        assert_eq!(classify_nal(&h264_nal(5, &[0; 3]), CpCodec::H264), CpNalKind::Keyframe);
        assert_eq!(classify_nal(&h264_nal(7, &[0; 3]), CpCodec::H264), CpNalKind::Params);
        assert_eq!(classify_nal(&h264_nal(8, &[0; 3]), CpCodec::H264), CpNalKind::Params);
        assert_eq!(classify_nal(&h264_nal(1, &[0; 3]), CpCodec::H264), CpNalKind::Delta);
    }

    #[test]
    fn h265_kinds() {
        assert_eq!(classify_nal(&h265_nal(19, &[0; 3]), CpCodec::H265), CpNalKind::Keyframe);
        assert_eq!(classify_nal(&h265_nal(33, &[0; 3]), CpCodec::H265), CpNalKind::Params);
        assert_eq!(classify_nal(&h265_nal(1, &[0; 3]), CpCodec::H265), CpNalKind::Delta);
    }

    #[test]
    fn the_most_significant_kind_wins() {
        let mut frame = h264_nal(1, &[0; 2]);
        frame.extend(h264_nal(7, &[0; 2]));
        frame.extend(h264_nal(5, &[0; 2]));
        frame.extend(h264_nal(1, &[0; 2]));
        assert_eq!(classify_nal(&frame, CpCodec::H264), CpNalKind::Keyframe);
    }

    #[test]
    fn a_broken_length_ends_the_walk() {
        assert_eq!(classify_nal(&[], CpCodec::H264), CpNalKind::Delta);
        assert_eq!(classify_nal(&[0, 0, 0], CpCodec::H264), CpNalKind::Delta);
        assert_eq!(classify_nal(&[0, 0, 0, 0, 5], CpCodec::H264), CpNalKind::Delta);

        // a keyframe announced longer than the frame is not counted
        let mut truncated = 99u32.to_be_bytes().to_vec();
        truncated.push(5);
        assert_eq!(classify_nal(&truncated, CpCodec::H264), CpNalKind::Delta);

        // and it ends the walk, so what follows stays unseen
        let mut frame = h264_nal(7, &[0; 2]);
        frame.extend_from_slice(&99u32.to_be_bytes());
        frame.extend(h264_nal(5, &[0; 2]));
        assert_eq!(classify_nal(&frame, CpCodec::H264), CpNalKind::Params);
    }
}
