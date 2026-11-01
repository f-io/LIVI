// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::server::{OP_GET_CERT, OP_PROTOCOL_MAJOR, OP_SIGN, STATUS_OK};
use crate::*;

pub struct NcmCoprocessor {
    addr: String,
    stream: Option<TcpStream>,
    protocol_major: Option<u8>,
}

impl NcmCoprocessor {
    pub fn new(addr: &str) -> Self {
        Self {
            addr: addr.to_string(),
            stream: None,
            protocol_major: None,
        }
    }

    fn ensure(&mut self) -> Result<&mut TcpStream, MfiError> {
        if self.stream.is_none() {
            let stream = livi_net::connect(self.addr.as_str(), IO_TIMEOUT)
                .map_err(|e| MfiError::Io(format!("connect {}: {e}", self.addr)))?;
            stream.set_nodelay(true).ok();
            stream
                .set_read_timeout(Some(IO_TIMEOUT + AUTH_TIMEOUT))
                .ok();
            stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
            self.stream = Some(stream);
        }
        Ok(self.stream.as_mut().expect("just connected"))
    }

    fn try_request(&mut self, req: &[u8]) -> Result<Vec<u8>, MfiError> {
        let result = self.exchange(req);
        if matches!(result, Err(MfiError::Io(_))) {
            self.stream = None;
        }
        result
    }

    fn exchange(&mut self, req: &[u8]) -> Result<Vec<u8>, MfiError> {
        let stream = self.ensure()?;
        stream
            .write_all(req)
            .map_err(|e| MfiError::Io(format!("mfid write: {e}")))?;
        let mut hdr = [0u8; 3];
        stream
            .read_exact(&mut hdr)
            .map_err(|e| MfiError::Io(format!("mfid read: {e}")))?;
        let len = ((hdr[1] as usize) << 8) | hdr[2] as usize;
        let mut data = vec![0u8; len];
        if len > 0 {
            stream
                .read_exact(&mut data)
                .map_err(|e| MfiError::Io(format!("mfid body: {e}")))?;
        }
        if hdr[0] != STATUS_OK {
            return Err(MfiError::AuthFailed { error_code: None });
        }
        Ok(data)
    }

    fn request(&mut self, req: &[u8]) -> Result<Vec<u8>, MfiError> {
        match self.try_request(req) {
            Err(MfiError::Io(first)) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.try_request(req).map_err(|e| match e {
                    MfiError::Io(second) => MfiError::Io(format!("{first}; retry: {second}")),
                    other => other,
                })
            }
            other => other,
        }
    }
}

impl AuthCoprocessor for NcmCoprocessor {
    fn protocol_major(&mut self) -> Result<u8, MfiError> {
        if let Some(v) = self.protocol_major {
            return Ok(v);
        }
        let d = self.request(&[OP_PROTOCOL_MAJOR])?;
        let v = *d
            .first()
            .ok_or_else(|| MfiError::Io("mfid: empty proto".into()))?;
        self.protocol_major = Some(v);
        Ok(v)
    }

    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        self.request(&[OP_GET_CERT])
    }

    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        let n = challenge.len();
        if !(CHALLENGE_MIN..=CHALLENGE_MAX).contains(&n) {
            return Err(MfiError::ChallengeSize(n));
        }
        let mut req = Vec::with_capacity(3 + n);
        req.push(OP_SIGN);
        req.push(((n >> 8) & 0xff) as u8);
        req.push((n & 0xff) as u8);
        req.extend_from_slice(challenge);
        self.request(&req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_socket_that_failed_is_not_used_again() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mfid = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 1];
            s.read_exact(&mut req).unwrap();
            s.write_all(&[STATUS_OK]).unwrap();
        });
        let mut chip = NcmCoprocessor::new(&addr);
        assert!(matches!(
            chip.try_request(&[OP_GET_CERT]),
            Err(MfiError::Io(_))
        ));
        assert!(chip.stream.is_none());
        mfid.join().unwrap();
    }
}
