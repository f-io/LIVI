//! The MFi coprocessor over TCP: the dongle's chip, lent to LIVI on the host. Server side of
//! `iap2_mfi::NcmCoprocessor`, which documents the wire.

use std::io::{Read, Write};

use iap2_mfi::{AuthCoprocessor, CHALLENGE_MAX, CHALLENGE_MIN};

pub const PORT: u16 = 5000;

const OP_GET_CERT: u8 = 0x01;
const OP_SIGN: u8 = 0x02;
const OP_PROTOCOL_MAJOR: u8 = 0x03;
const STATUS_OK: u8 = 0;
const STATUS_ERR: u8 = 1;
/// An RSA (2.x) certificate runs to ~945 B, an ECDSA (3.0) one to ~608 B.
const CERT_LEN_SPLIT: usize = 768;

/// Serves one client until it closes or sends something we do not understand.
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
                    // A length we will not read is a framing error, not a failed request.
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

/// Register 0x02 reads back as rubbish on the 2.0B chip once it has signed something. The
/// certificate length says which generation it is (2 = SHA-1, 3 = SHA-256).
fn protocol_major(chip: &mut dyn AuthCoprocessor) -> Option<u8> {
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

#[cfg(target_os = "linux")]
pub fn run(args: &[String]) -> std::process::ExitCode {
    use iap2_mfi::I2cCoprocessor;
    use std::net::TcpListener;

    // The bus, as `/dev/i2c-<n>` or a bare number. The chip is externally powered, no GPIO.
    let bus = args
        .first()
        .map(|a| a.trim_start_matches("/dev/i2c-"))
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(1);
    let mut chip = match I2cCoprocessor::open(bus, -1) {
        Ok(chip) => chip,
        Err(e) => {
            eprintln!("[mfid] no MFi chip on i2c-{bus}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let address = chip.address();
    let major = protocol_major(&mut chip)
        .map(|m| m.to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("[mfid] MFi @0x{address:02X} on i2c-{bus}, protocol major {major}");

    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[mfid] bind :{PORT}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("[mfid] listening on :{PORT}");
    // One client at a time.
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        serve(&mut stream, &mut chip);
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use iap2_mfi::MfiError;

    struct FakeChip {
        major: Result<u8, MfiError>,
        cert: Vec<u8>,
        signed: Vec<Vec<u8>>,
    }

    impl FakeChip {
        fn new(major: Result<u8, MfiError>, cert_len: usize) -> Self {
            Self {
                major,
                cert: vec![0xAB; cert_len],
                signed: Vec::new(),
            }
        }
    }

    impl AuthCoprocessor for FakeChip {
        fn protocol_major(&mut self) -> Result<u8, MfiError> {
            match &self.major {
                Ok(v) => Ok(*v),
                Err(_) => Err(MfiError::Io("no answer".into())),
            }
        }
        fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
            Ok(self.cert.clone())
        }
        fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
            self.signed.push(challenge.to_vec());
            Ok(vec![0x5A; 128])
        }
    }

    /// A client connection: what it sends, and what it gets back.
    struct Wire {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Wire {
        fn new(input: Vec<u8>) -> Self {
            Self {
                input: std::io::Cursor::new(input),
                output: Vec::new(),
            }
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn responses(output: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 3 <= output.len() {
            let len = usize::from(u16::from_be_bytes([output[i + 1], output[i + 2]]));
            out.push((output[i], output[i + 3..i + 3 + len].to_vec()));
            i += 3 + len;
        }
        out
    }

    #[test]
    fn serves_certificate_and_signature_on_one_connection() {
        let mut chip = FakeChip::new(Ok(2), 945);
        let mut wire = Wire::new(vec![OP_GET_CERT, OP_SIGN, 0, 4, 1, 2, 3, 4]);
        serve(&mut wire, &mut chip);
        let got = responses(&wire.output);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], (STATUS_OK, vec![0xAB; 945]));
        assert_eq!(got[1].0, STATUS_OK);
        assert_eq!(got[1].1.len(), 128);
        assert_eq!(chip.signed, vec![vec![1, 2, 3, 4]]);
    }

    #[test]
    fn reports_the_generation_the_chip_states() {
        let mut chip = FakeChip::new(Ok(3), 608);
        let mut wire = Wire::new(vec![OP_PROTOCOL_MAJOR]);
        serve(&mut wire, &mut chip);
        assert_eq!(responses(&wire.output), vec![(STATUS_OK, vec![3])]);
    }

    #[test]
    fn falls_back_to_the_cert_length_when_the_register_misreads() {
        // 0xCD is what the 2.0B returns after it has signed something.
        for (register, cert_len, expected) in [
            (Ok(0xCD), 945, 2),
            (Ok(0xCD), 608, 3),
            (Err(MfiError::Io("x".into())), 945, 2),
        ] {
            let mut chip = FakeChip::new(register, cert_len);
            let mut wire = Wire::new(vec![OP_PROTOCOL_MAJOR]);
            serve(&mut wire, &mut chip);
            assert_eq!(responses(&wire.output), vec![(STATUS_OK, vec![expected])]);
        }
    }

    #[test]
    fn refuses_a_challenge_that_is_not_a_challenge() {
        let mut chip = FakeChip::new(Ok(2), 945);
        let mut wire = Wire::new(vec![OP_SIGN, 0x04, 0x00]); // 1024 bytes
        serve(&mut wire, &mut chip);
        assert_eq!(responses(&wire.output), vec![(STATUS_ERR, vec![])]);
        assert!(chip.signed.is_empty());
    }

    #[test]
    fn a_truncated_request_ends_the_connection_without_an_answer() {
        let mut chip = FakeChip::new(Ok(2), 945);
        let mut wire = Wire::new(vec![OP_SIGN, 0]);
        serve(&mut wire, &mut chip);
        assert!(wire.output.is_empty());
    }
}
