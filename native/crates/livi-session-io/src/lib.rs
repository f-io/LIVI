// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "android")]
extern crate nusb_android as nusb;

pub mod feed;
#[cfg(usb_host)]
pub mod hotplug;
pub mod line;
pub mod link;
pub mod sock;
#[cfg_attr(not(usb_host), path = "usb_absent.rs")]
pub mod usb;
