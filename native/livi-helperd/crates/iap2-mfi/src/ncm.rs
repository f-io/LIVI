//! Remote MFi coprocessor: the chip in the LIVI Link dongle, reached over TCP via its `mfid`.
//!
//! Wire protocol (matches mfid), big-endian lengths, multiple requests per socket:
//!   GET_CERT   : [0x01]                              -> [status][len:2][cert]
//!   SIGN       : [0x02][len:2][challenge]            -> [status][len:2][signature]
//!   PROTO_MAJOR: [0x03]                              -> [status][len:2][major:1]

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

use crate::*;

pub struct NcmCoprocessor {
    addr: String,
    stream: Option<TcpStream>,
    protocol_major: Option<u8>,
}

/// The port the dongle's `mfid` listens on.
pub const DEFAULT_PORT: u16 = 5000;

impl NcmCoprocessor {
    /// Names the dongle's `mfid` ("host:port"); connects on first use.
    pub fn new(addr: &str) -> Self {
        Self { addr: addr.to_string(), stream: None, protocol_major: None }
    }

    /// As `new`, but connects immediately.
    pub fn connect(addr: &str) -> Result<Self, MfiError> {
        let mut chip = Self::new(addr);
        chip.ensure()?;
        Ok(chip)
    }

    fn ensure(&mut self) -> Result<&mut TcpStream, MfiError> {
        if self.stream.is_none() {
            // Connects with a timeout; an unreachable dongle fails within IO_TIMEOUT.
            let sockaddr = self
                .addr
                .to_socket_addrs()
                .map_err(|e| MfiError::Io(format!("resolve {}: {e}", self.addr)))?
                .next()
                .ok_or_else(|| MfiError::Io(format!("resolve {}: no address", self.addr)))?;
            let stream = TcpStream::connect_timeout(&sockaddr, IO_TIMEOUT)
                .map_err(|e| MfiError::Io(format!("connect {}: {e}", self.addr)))?;
            stream.set_nodelay(true).ok();
            stream.set_read_timeout(Some(IO_TIMEOUT + AUTH_TIMEOUT)).ok();
            stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
            self.stream = Some(stream);
        }
        Ok(self.stream.as_mut().expect("just connected"))
    }

    fn try_request(&mut self, req: &[u8]) -> Result<Vec<u8>, MfiError> {
        let stream = self.ensure()?;
        stream.write_all(req).map_err(|e| MfiError::Io(format!("mfid write: {e}")))?;
        let mut hdr = [0u8; 3];
        stream.read_exact(&mut hdr).map_err(|e| MfiError::Io(format!("mfid read: {e}")))?;
        let len = ((hdr[1] as usize) << 8) | hdr[2] as usize;
        let mut data = vec![0u8; len];
        if len > 0 {
            stream.read_exact(&mut data).map_err(|e| MfiError::Io(format!("mfid body: {e}")))?;
        }
        if hdr[0] != 0 {
            return Err(MfiError::AuthFailed { error_code: None });
        }
        Ok(data)
    }

    fn request(&mut self, req: &[u8]) -> Result<Vec<u8>, MfiError> {
        match self.try_request(req) {
            Err(MfiError::Io(first)) => {
                // Dead socket: waits briefly, reconnects once.
                self.stream = None;
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
        let d = self.request(&[0x03])?;
        let v = *d.first().ok_or_else(|| MfiError::Io("mfid: empty proto".into()))?;
        self.protocol_major = Some(v);
        Ok(v)
    }

    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        self.request(&[0x01])
    }

    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        let n = challenge.len();
        if !(CHALLENGE_MIN..=CHALLENGE_MAX).contains(&n) {
            return Err(MfiError::ChallengeSize(n));
        }
        let mut req = Vec::with_capacity(3 + n);
        req.push(0x02);
        req.push(((n >> 8) & 0xff) as u8);
        req.push((n & 0xff) as u8);
        req.extend_from_slice(challenge);
        self.request(&req)
    }
}
