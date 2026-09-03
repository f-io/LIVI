pub mod aa_sock;
pub mod bluetoothd;
pub mod bonjour;
pub mod bringup;
pub mod clock;
pub mod driver;
pub mod events;
pub mod file_transfer;
pub mod framing;
pub mod hfp;
#[cfg(target_os = "linux")]
pub mod sco;
pub mod wifi_ap;
pub mod ident;
pub mod livi_sock;
pub mod net;
pub mod reconnect;
pub mod state;

#[cfg(target_os = "linux")]
pub mod bt;
#[cfg(target_os = "linux")]
pub mod mfi_async;

use std::future::Future;

/// A bidirectional stream of whole CSM control-session frames.
pub trait ControlChannel {
    fn send(&mut self, frame: Vec<u8>) -> impl Future<Output = Result<(), ChannelError>> + Send;
    fn recv(&mut self) -> impl Future<Output = Option<Vec<u8>>> + Send;
}

/// Certificate read and challenge signing, async.
pub trait AsyncAuth {
    fn read_certificate(&mut self) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    fn sign(&mut self, challenge: Vec<u8>) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    fn protocol_major(&mut self) -> impl Future<Output = Result<u8, String>> + Send;
}

#[derive(Debug)]
pub enum ChannelError {
    Closed,
}

impl core::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "control channel closed")
    }
}

impl std::error::Error for ChannelError {}
