// USB access for CarPlay: selects the phone's CarPlay configuration without a role switch.
// The phone sits on local usbfs or on a LIVI Link dongle; `backend::set_remote` picks which.

pub const APPLE_VID: u16 = 0x05ac;
pub const CP_CONFIG: u8 = 6;

pub mod ntb;

mod pipe;
pub use pipe::{MuxReader, MuxWriter, PhoneInfo};

mod backend;
pub use backend::{
    ensure_carplay_config, find_iphones, open_pipes, remote_addr, restore_default_config,
    try_find_iphones,
    set_remote,
};

pub mod remote;

pub const EP_OUT: u8 = 0x04;
pub const EP_IN: u8 = 0x85;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{open_by_address, restore_all_default_config, IPhoneDev};

#[cfg(target_os = "linux")]
mod usb_pipe;

mod mux;
pub use mux::{MuxHost, MuxTcpConn, LOCKDOWN_PORT};

mod device;
pub use device::{socket_path, MuxDevice, MuxRegistry};

#[cfg(target_os = "linux")]
mod ncm;
#[cfg(target_os = "linux")]
pub use ncm::NcmBridge;

mod async_stream;
pub use async_stream::AsyncMuxStream;
