// Where the phone hangs: local usbfs, or a LIVI Link dongle serving the same operations over TCP.

use std::sync::RwLock;

use crate::pipe::{MuxPipes, PhoneInfo};

static REMOTE: RwLock<Option<String>> = RwLock::new(None);

/// Route phone lookups and pipes through the dongle at `host:port`.
pub fn set_remote(addr: &str) {
    *REMOTE.write().unwrap() = Some(addr.to_string());
}

pub fn remote_addr() -> Option<String> {
    if let Some(a) = REMOTE.read().unwrap().clone() {
        return Some(a);
    }
    std::env::var("LIVI_USBMUX_REMOTE").ok().filter(|s| !s.trim().is_empty())
}

pub fn find_iphones() -> Vec<PhoneInfo> {
    try_find_iphones().unwrap_or_default()
}

/// Separates "the backend did not answer" from "it answered with no phone".
pub fn try_find_iphones() -> Result<Vec<PhoneInfo>, String> {
    match remote_addr() {
        Some(addr) => crate::remote::try_find_iphones(&addr),
        None => Ok(local::find_iphones()),
    }
}

pub fn ensure_carplay_config(serial: &str) -> Result<PhoneInfo, String> {
    match remote_addr() {
        Some(addr) => crate::remote::ensure_carplay_config(&addr, serial),
        None => local::ensure_carplay_config(serial),
    }
}

pub fn restore_default_config(serial: &str) {
    match remote_addr() {
        Some(addr) => crate::remote::restore_default_config(&addr, serial),
        None => local::restore_default_config(serial),
    }
}

pub fn open_pipes(serial: &str) -> Result<MuxPipes, String> {
    match remote_addr() {
        Some(addr) => crate::remote::open_pipes(&addr, serial),
        None => local::open_pipes(serial),
    }
}

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

    pub fn open_pipes(serial: &str) -> Result<MuxPipes, String> {
        crate::usb_pipe::open_pipes(serial)
    }
}

// No usbfs here: the phone can only be reached through a dongle.
#[cfg(not(target_os = "linux"))]
mod local {
    use super::*;

    const NO_LOCAL: &str = "no local USB backend on this platform — set the LIVI Link address";

    pub fn find_iphones() -> Vec<PhoneInfo> {
        Vec::new()
    }

    pub fn ensure_carplay_config(_serial: &str) -> Result<PhoneInfo, String> {
        Err(NO_LOCAL.into())
    }

    pub fn restore_default_config(_serial: &str) {}

    pub fn open_pipes(_serial: &str) -> Result<MuxPipes, String> {
        Err(NO_LOCAL.into())
    }
}
