// MFi authentication coprocessor: reads the accessory certificate and signs challenges.

use std::time::Duration;

pub const CHALLENGE_MIN: usize = 1;
pub const CHALLENGE_MAX: usize = 128;

pub const REG_DEVICE_VERSION: u8 = 0x00;
pub const REG_PROTOCOL_MAJOR: u8 = 0x02;
pub const REG_ERROR_CODE: u8 = 0x05;
pub const REG_AUTH_CONTROL_STATUS: u8 = 0x10;
pub const REG_SIGNATURE_LENGTH: u8 = 0x11;
pub const REG_SIGNATURE_DATA: u8 = 0x12;
pub const REG_CHALLENGE_LENGTH: u8 = 0x20;
pub const REG_CHALLENGE_DATA: u8 = 0x21;
pub const REG_CERT_LENGTH: u8 = 0x30;
pub const REG_CERT_DATA: u8 = 0x31;

pub const AUTH_START: u8 = 0x01;
pub const AUTH_DONE: u8 = 0x10;

pub const DEV_ADDR_CANDIDATES: [u16; 2] = [0x10, 0x11];

#[derive(Debug)]
pub enum MfiError {
    Timeout(String),
    ChallengeSize(usize),
    AuthFailed { error_code: Option<u8> },
    NoChip { probed: Vec<u16> },
    Io(String),
}

impl core::fmt::Display for MfiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MfiError::Timeout(what) => write!(f, "timeout during {what}"),
            MfiError::ChallengeSize(n) => write!(f, "challenge must be 1..=128 bytes, got {n}"),
            MfiError::AuthFailed { error_code: Some(c) } => {
                write!(f, "auth failed (error code 0x{c:02X})")
            }
            MfiError::AuthFailed { error_code: None } => write!(f, "auth failed (error unreadable)"),
            MfiError::NoChip { probed } => {
                let addrs: Vec<String> = probed.iter().map(|a| format!("0x{a:02X}")).collect();
                write!(f, "no coprocessor answered at {}", addrs.join("/"))
            }
            MfiError::Io(e) => write!(f, "i2c io: {e}"),
        }
    }
}

impl std::error::Error for MfiError {}

/// The MFi coprocessor, on a local i2c bus or behind the STM bridge.
pub trait AuthCoprocessor {
    fn protocol_major(&mut self) -> Result<u8, MfiError>;
    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError>;
    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError>;
}

/// No coprocessor at hand: every operation fails.
pub struct NoCoprocessor;

impl AuthCoprocessor for NoCoprocessor {
    fn protocol_major(&mut self) -> Result<u8, MfiError> {
        Err(MfiError::Io("no MFi coprocessor".into()))
    }
    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        Err(MfiError::Io("no MFi coprocessor".into()))
    }
    fn generate_challenge_response(&mut self, _challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        Err(MfiError::Io("no MFi coprocessor".into()))
    }
}

pub const BUSY_RETRY: Duration = Duration::from_micros(500);
pub const IO_TIMEOUT: Duration = Duration::from_secs(2);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub const AUTH_POLL: Duration = Duration::from_millis(10);
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::I2cCoprocessor;

// Remote coprocessor over TCP (LIVI Link dongle across the USB-NCM link).
pub mod ncm;
pub use ncm::NcmCoprocessor;
