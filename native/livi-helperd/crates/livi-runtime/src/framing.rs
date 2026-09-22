use iap2_csm::CSM_START;

/// Splits the control-session byte stream into whole CSM frames.
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        while self.buf.len() >= 2
            && u16::from_be_bytes([self.buf[0], self.buf[1]]) != CSM_START
        {
            self.buf.remove(0);
        }
        if self.buf.len() < 6 {
            return None;
        }
        let len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
        if len < 6 {
            self.buf.remove(0);
            return self.next_frame();
        }
        if self.buf.len() < len {
            return None;
        }
        Some(self.buf.drain(..len).collect())
    }
}

pub fn frame_msg_id(frame: &[u8]) -> Option<u16> {
    if frame.len() < 6 {
        return None;
    }
    Some(u16::from_be_bytes([frame[4], frame[5]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_concatenated_frames() {
        let mut r = FrameReader::default();
        r.push(&[0x40, 0x40, 0x00, 0x06, 0xA1, 0x00, 0x40, 0x40, 0x00, 0x06, 0x1D, 0x00]);
        assert_eq!(frame_msg_id(&r.next_frame().unwrap()), Some(0xA100));
        assert_eq!(frame_msg_id(&r.next_frame().unwrap()), Some(0x1D00));
        assert_eq!(r.next_frame(), None);
    }

    #[test]
    fn waits_for_full_frame() {
        let mut r = FrameReader::default();
        r.push(&[0x40, 0x40, 0x00, 0x08, 0xAA, 0x01]);
        assert_eq!(r.next_frame(), None);
        r.push(&[0xDE, 0xAD]);
        assert_eq!(r.next_frame().unwrap(), vec![0x40, 0x40, 0x00, 0x08, 0xAA, 0x01, 0xDE, 0xAD]);
    }

    #[test]
    fn resyncs_past_garbage() {
        let mut r = FrameReader::default();
        r.push(&[0x00, 0xFF, 0x40, 0x40, 0x00, 0x06, 0x1D, 0x02]);
        assert_eq!(frame_msg_id(&r.next_frame().unwrap()), Some(0x1D02));
    }
}
