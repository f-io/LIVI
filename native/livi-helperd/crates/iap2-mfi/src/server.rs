//! LIVI-Link MFi wire (paired with [`crate::NcmCoprocessor`]):
//!   GET_CERT    : [0x01]                       -> [status][len:2][cert]
//!   SIGN        : [0x02][len:2][challenge]     -> [status][len:2][signature]
//!   PROTO_MAJOR : [0x03]                       -> [status][len:2][major:1]

use std::io::{Read, Write};

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
