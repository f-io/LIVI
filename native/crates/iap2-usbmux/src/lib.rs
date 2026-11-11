// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "android")]
extern crate nusb_android as nusb;

pub const APPLE_VID: u16 = 0x05ac;
pub const CP_CONFIG: u8 = 6;
#[cfg(any(target_os = "linux", target_os = "android"))]
const DEFAULT_CONFIG: u8 = 4;

mod error;
pub use error::Error;

pub mod ntb;

mod pipe;
pub use pipe::PhoneInfo;

mod backend;
pub use backend::{ensure_carplay_config, find_iphones, reset, restore_default_config};

#[cfg(any(target_os = "linux", target_os = "android", test))]
pub(crate) type Mac = [u8; 6];

pub(crate) fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}

pub const EP_OUT: u8 = 0x04;
pub const EP_IN: u8 = 0x85;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{IPhoneDev, open_by_address, restore_all_default_config};

#[cfg(target_os = "android")]
mod android;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod usb_pipe;

mod mux;
pub use mux::{LOCKDOWN_PORT, MuxHost, MuxStream};

mod device;
pub use device::{MuxDevice, MuxRegistry, socket_path};

#[cfg(any(target_os = "android", test))]
mod ether;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod ncm;
#[cfg(target_os = "android")]
pub use ether::{FromPhone, Ip};
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use ncm::NcmBridge;
#[cfg(target_os = "linux")]
pub use ncm::drop_name_bound_profiles;
#[cfg(target_os = "android")]
pub use ncm::{Link, OpenTun, Port, TunRequest, Tunnel};
