// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const APPLE_VID: u16 = 0x05ac;
pub const CP_CONFIG: u8 = 6;

pub mod ntb;

mod pipe;
pub use pipe::{MuxReader, MuxWriter, PhoneInfo};

mod backend;
pub use backend::{ensure_carplay_config, find_iphones, open_pipes, reset, restore_default_config};

pub const EP_OUT: u8 = 0x04;
pub const EP_IN: u8 = 0x85;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{IPhoneDev, open_by_address, restore_all_default_config};

#[cfg(target_os = "linux")]
mod usb_pipe;

mod mux;
pub use mux::{LOCKDOWN_PORT, MuxHost, MuxTcpConn};

mod device;
pub use device::{MuxDevice, MuxRegistry, socket_path};

#[cfg(target_os = "linux")]
mod ncm;
#[cfg(target_os = "linux")]
pub use ncm::{NcmBridge, drop_name_bound_profiles};

mod async_stream;
pub use async_stream::AsyncMuxStream;
