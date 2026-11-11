// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub use local::{ensure_carplay_config, find_iphones, open_stream, reset, restore_default_config};

#[cfg(target_os = "linux")]
mod local {
    use crate::Error;
    use crate::pipe::PhoneInfo;

    pub use crate::usb_pipe::open_stream;

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

    pub fn ensure_carplay_config(serial: &str) -> Result<PhoneInfo, Error> {
        crate::linux::ensure_carplay_config(serial).map(|d| PhoneInfo {
            serial: d.serial,
            num_configs: d.num_configs,
            config_value: d.config_value,
        })
    }

    pub fn restore_default_config(serial: &str) {
        crate::linux::restore_default_config(serial)
    }

    pub fn reset(serial: &str) -> Result<(), Error> {
        crate::linux::reset(serial)
    }
}

#[cfg(target_os = "android")]
mod local {
    pub use crate::android::{ensure_carplay_config, find_iphones, reset, restore_default_config};
    pub use crate::usb_pipe::open_stream;
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod local {
    use livi_session_io::usb::UsbStream;

    use crate::Error;
    use crate::pipe::PhoneInfo;

    pub fn find_iphones() -> Vec<PhoneInfo> {
        Vec::new()
    }

    pub fn ensure_carplay_config(_serial: &str) -> Result<PhoneInfo, Error> {
        Err(Error::Unsupported)
    }

    pub fn restore_default_config(_serial: &str) {}

    pub fn reset(_serial: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }

    pub async fn open_stream(_serial: &str) -> Result<UsbStream, Error> {
        Err(Error::Unsupported)
    }
}
