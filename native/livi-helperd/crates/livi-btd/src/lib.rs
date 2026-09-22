// LIVI-Link BT tunnel: hci0 handed to the host over TCP via HCI_CHANNEL_USER.
//
// Wire: 2-byte big-endian length + raw HCI packet, both directions.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub mod hci;

#[cfg(not(target_os = "linux"))]
pub fn run() -> std::process::ExitCode {
    eprintln!("[btd] linux only");
    std::process::ExitCode::FAILURE
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> std::process::ExitCode {
    eprintln!("[btd] linux only");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{PORT, probe, run};
