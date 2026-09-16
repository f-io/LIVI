// LIVI-Link iAP2 accessory daemon: BlueZ mgmt, SDP, RFCOMM channel + host handoff.
// Control :5005, session :5004.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub mod mgmt;
pub mod sdp;

#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
pub use server::{Config, NameSource, PORT, CONTROL_PORT, run};

#[cfg(not(target_os = "linux"))]
pub const PORT: u16 = 5004;
#[cfg(not(target_os = "linux"))]
pub const CONTROL_PORT: u16 = 5005;
