// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::pipe::{MuxPipes, PhoneInfo};

pub use local::{ensure_carplay_config, find_iphones, open_pipes, reset, restore_default_config};

#[cfg(target_os = "linux")]
mod local {
    use super::*;

    pub fn find_iphones() -> Vec<PhoneInfo> {
        crate::linux::find_iphones()
            .into_iter()
            .map(|d| PhoneInfo {
                serial: d.serial,
                num_configs: d.num_configs,
                config_value: d.config_value,
            })
            .collect()
    }

    pub fn ensure_carplay_config(serial: &str) -> Result<PhoneInfo, String> {
        crate::linux::ensure_carplay_config(serial).map(|d| PhoneInfo {
            serial: d.serial,
            num_configs: d.num_configs,
            config_value: d.config_value,
        })
    }

    pub fn restore_default_config(serial: &str) {
        crate::linux::restore_default_config(serial)
    }

    pub fn reset(serial: &str) -> Result<(), String> {
        crate::linux::reset(serial)
    }

    pub fn open_pipes(serial: &str) -> Result<MuxPipes, String> {
        crate::usb_pipe::open_pipes(serial)
    }
}

#[cfg(not(target_os = "linux"))]
mod local {
    use super::*;

    const NO_LOCAL: &str = "no local USB backend on this platform";

    pub fn find_iphones() -> Vec<PhoneInfo> {
        Vec::new()
    }

    pub fn ensure_carplay_config(_serial: &str) -> Result<PhoneInfo, String> {
        Err(NO_LOCAL.into())
    }

    pub fn restore_default_config(_serial: &str) {}

    pub fn reset(_serial: &str) -> Result<(), String> {
        Err(NO_LOCAL.into())
    }

    pub fn open_pipes(_serial: &str) -> Result<MuxPipes, String> {
        Err(NO_LOCAL.into())
    }
}
