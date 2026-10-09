//! A Bluetooth host for the controller behind the dongle's HCI tunnel, where no BlueZ drives it.

pub mod hci;
mod host;
pub mod keys;
pub mod l2cap;
pub mod rfcomm;
pub mod sdp;

pub use host::{Call, Config, Inbox, Incoming, Notice, Stack};
