//! LIVI-Link MFi wire (paired with [`crate::NcmCoprocessor`]):
//!   GET_CERT    : [0x01]                       -> [status][len:2][cert]
//!   SIGN        : [0x02][len:2][challenge]     -> [status][len:2][signature]
//!   PROTO_MAJOR : [0x03]                       -> [status][len:2][major:1]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use crate::{AuthCoprocessor, CHALLENGE_MAX, CHALLENGE_MIN, MfiError};

pub const PORT: u16 = 5000;

pub const OP_GET_CERT: u8 = 0x01;
pub const OP_SIGN: u8 = 0x02;
pub const OP_PROTOCOL_MAJOR: u8 = 0x03;

pub const STATUS_OK: u8 = 0;
pub const STATUS_ERR: u8 = 1;

/// An RSA (2.x) certificate runs to ~945 B, an ECDSA (3.0) one to ~608 B.
/// Used as a cross-check when register 0x02 reads back as garbage.
pub const CERT_LEN_SPLIT: usize = 768;

pub fn serve<S: Read + Write>(io: &mut S, chip: &mut dyn AuthCoprocessor) {
    loop {
        let mut op = [0u8; 1];
        if io.read_exact(&mut op).is_err() {
            return;
        }
        let answered = match op[0] {
            OP_GET_CERT => match chip.read_certificate() {
                Ok(cert) => respond(io, STATUS_OK, &cert),
                Err(e) => {
                    eprintln!("[mfid] certificate: {e}");
                    respond(io, STATUS_ERR, &[])
                }
            },
            OP_SIGN => {
                let mut len = [0u8; 2];
                if io.read_exact(&mut len).is_err() {
                    return;
                }
                let len = usize::from(u16::from_be_bytes(len));
                if !(CHALLENGE_MIN..=CHALLENGE_MAX).contains(&len) {
                    // A length we will not read is a framing error, not
                    // a failed request — close the connection.
                    let _ = respond(io, STATUS_ERR, &[]);
                    return;
                }
                let mut challenge = vec![0u8; len];
                if io.read_exact(&mut challenge).is_err() {
                    return;
                }
                match chip.generate_challenge_response(&challenge) {
                    Ok(sig) => respond(io, STATUS_OK, &sig),
                    Err(e) => {
                        eprintln!("[mfid] sign: {e}");
                        respond(io, STATUS_ERR, &[])
                    }
                }
            }
            OP_PROTOCOL_MAJOR => match protocol_major(chip) {
                Some(major) => respond(io, STATUS_OK, &[major]),
                None => respond(io, STATUS_ERR, &[]),
            },
            _ => return,
        };
        if answered.is_err() {
            return;
        }
    }
}

struct Shared<C>(Arc<Mutex<C>>);

impl<C: AuthCoprocessor> Shared<C> {
    fn with<T>(&self, f: impl FnOnce(&mut C) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl<C: AuthCoprocessor> AuthCoprocessor for Shared<C> {
    fn protocol_major(&mut self) -> Result<u8, MfiError> {
        self.with(|c| c.protocol_major())
    }

    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        self.with(|c| c.read_certificate())
    }

    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        self.with(|c| c.generate_challenge_response(challenge))
    }
}

/// Serves each connection on a thread of its own.
pub fn listen<C: AuthCoprocessor + Send + 'static>(listener: TcpListener, chip: C) {
    let chip = Arc::new(Mutex::new(chip));
    for mut stream in listener.incoming().flatten() {
        let mut chip = Shared(chip.clone());
        std::thread::spawn(move || {
            keepalive(&stream);
            serve(&mut stream, &mut chip);
        });
    }
}

/// Ends the thread of a client that vanished, after 3 s.
#[cfg(target_os = "linux")]
fn keepalive(stream: &TcpStream) {
    use std::os::fd::AsRawFd;

    for (level, name, value) in [
        (libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 2),
    ] {
        let value: libc::c_int = value;
        let rc = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                level,
                name,
                (&raw const value).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            eprintln!("[mfid] keepalive: {}", std::io::Error::last_os_error());
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn keepalive(_stream: &TcpStream) {}

/// Best-effort major-version resolution. Register 0x02 reads back as
/// rubbish on the 2.0B chip once it has signed something, so we cross-check
/// against the certificate length (2.x ≈ 945 B, 3.0 ≈ 608 B).
pub fn protocol_major(chip: &mut dyn AuthCoprocessor) -> Option<u8> {
    match chip.protocol_major() {
        Ok(major @ (2 | 3)) => Some(major),
        other => {
            if let Ok(major) = other {
                eprintln!("[mfid] protocol major reads as 0x{major:02X}, using the cert length");
            }
            let cert = chip.read_certificate().ok()?;
            Some(if cert.len() < CERT_LEN_SPLIT { 3 } else { 2 })
        }
    }
}

fn respond<S: Write>(io: &mut S, status: u8, data: &[u8]) -> std::io::Result<()> {
    let mut msg = Vec::with_capacity(3 + data.len());
    msg.push(status);
    msg.extend_from_slice(&(data.len() as u16).to_be_bytes());
    msg.extend_from_slice(data);
    io.write_all(&msg)
}

// Kept for symmetry with the NcmCoprocessor's public error surface —
// callers may want to bubble a serve() failure up as MfiError::Io.
#[allow(dead_code)]
fn wrap_io(e: std::io::Error) -> MfiError {
    MfiError::Io(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Chip;

    impl AuthCoprocessor for Chip {
        fn protocol_major(&mut self) -> Result<u8, MfiError> {
            Ok(3)
        }

        fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
            Ok(vec![0xAA; 4])
        }

        fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
            Ok(challenge.to_vec())
        }
    }

    #[test]
    fn a_silent_connection_does_not_hold_up_the_next() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || listen(listener, Chip));

        let _silent = TcpStream::connect(addr).unwrap();
        let mut client = TcpStream::connect(addr).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        client.write_all(&[OP_PROTOCOL_MAJOR]).unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply, [STATUS_OK, 0, 1, 3]);
    }
}
