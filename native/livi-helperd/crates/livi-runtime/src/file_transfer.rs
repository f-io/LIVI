// Reassembles files the phone pushes over the iAP2 file-transfer session (album artwork).

use std::collections::HashMap;

const SETUP: u8 = 0x04;
const START: u8 = 0x01;
const FIRST_DATA: u8 = 0x80;
const FIRST_AND_ONLY_DATA: u8 = 0xC0;
const DATA: u8 = 0x00;
const LAST_DATA: u8 = 0x40;
const CANCEL: u8 = 0x02;
const SUCCESS: u8 = 0x05;

pub enum FtOutput {
    Reply(Vec<u8>),
    Complete(Vec<u8>),
}

#[derive(Default)]
pub struct FileTransferReceiver {
    buffers: HashMap<u8, Vec<u8>>,
}

impl FileTransferReceiver {
    pub fn feed(&mut self, datagram: &[u8]) -> Vec<FtOutput> {
        if datagram.len() < 2 {
            return Vec::new();
        }
        let ftid = datagram[0];
        let ctrl = datagram[1];
        let data = &datagram[2..];
        match ctrl {
            SETUP => {
                self.buffers.insert(ftid, Vec::new());
                vec![FtOutput::Reply(vec![ftid, START])]
            }
            FIRST_DATA => {
                self.buffers.insert(ftid, data.to_vec());
                Vec::new()
            }
            DATA => {
                self.buffers.entry(ftid).or_default().extend_from_slice(data);
                Vec::new()
            }
            FIRST_AND_ONLY_DATA => self.complete(ftid, data.to_vec()),
            LAST_DATA => {
                let mut buf = self.buffers.remove(&ftid).unwrap_or_default();
                buf.extend_from_slice(data);
                self.complete(ftid, buf)
            }
            CANCEL => {
                self.buffers.remove(&ftid);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn complete(&mut self, ftid: u8, data: Vec<u8>) -> Vec<FtOutput> {
        self.buffers.remove(&ftid);
        vec![FtOutput::Reply(vec![ftid, SUCCESS]), FtOutput::Complete(data)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_datagram_completes() {
        let mut r = FileTransferReceiver::default();
        let out = r.feed(&[7, FIRST_AND_ONLY_DATA, 0xAA, 0xBB]);
        assert!(matches!(out[0], FtOutput::Reply(ref b) if b == &[7, SUCCESS]));
        assert!(matches!(&out[1], FtOutput::Complete(d) if d == &[0xAA, 0xBB]));
    }

    #[test]
    fn multi_part_reassembles() {
        let mut r = FileTransferReceiver::default();
        assert!(matches!(r.feed(&[3, SETUP])[0], FtOutput::Reply(ref b) if b == &[3, START]));
        assert!(r.feed(&[3, FIRST_DATA, 1, 2]).is_empty());
        assert!(r.feed(&[3, DATA, 3, 4]).is_empty());
        let out = r.feed(&[3, LAST_DATA, 5]);
        assert!(matches!(&out[1], FtOutput::Complete(d) if d == &[1, 2, 3, 4, 5]));
    }
}
