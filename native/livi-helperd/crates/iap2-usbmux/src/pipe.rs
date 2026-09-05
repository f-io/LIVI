// The mux's two byte pipes, the CarPlay bulk OUT and IN endpoints: local usbfs endpoints or a
// LIVI Link dongle serving them over TCP.

use std::time::Duration;

pub trait MuxWriter: Send {
    fn write(&mut self, data: &[u8]) -> Result<(), String>;
}

pub trait MuxReader: Send {
    /// One chunk, or empty on timeout. The mux framing re-assembles packets itself.
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, String>;
}

/// The two halves of one phone's mux pipe.
pub type MuxPipes = (Box<dyn MuxWriter>, Box<dyn MuxReader>);

/// A phone as far as the mux layer cares.
#[derive(Debug, Clone)]
pub struct PhoneInfo {
    pub serial: String,
    pub num_configs: u8,
    pub config_value: Option<u8>,
}
