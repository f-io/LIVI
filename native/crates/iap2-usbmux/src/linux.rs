// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::path::{Path, PathBuf};

use nusb::MaybeFuture;

use crate::usb_pipe::await_carplay_configs;
use crate::{APPLE_VID, CP_CONFIG, DEFAULT_CONFIG, Error};

const SYSFS_USB: &str = "/sys/bus/usb/devices";

#[derive(Debug, Clone)]
pub struct IPhoneDev {
    pub serial: String,
    pub sysfs: PathBuf,
    pub bus: u8,
    pub address: u8,
    pub num_configs: u8,
    pub config_value: Option<u8>,
}

fn read_trim(path: &Path, name: &str) -> Option<String> {
    fs::read_to_string(path.join(name))
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn find_iphones() -> Vec<IPhoneDev> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(SYSFS_USB) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if read_trim(&path, "idVendor").as_deref() != Some("05ac") {
            continue;
        }
        let Some(serial) = read_trim(&path, "serial").filter(|s| !s.is_empty()) else {
            continue;
        };
        let bus = read_trim(&path, "busnum")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let address = read_trim(&path, "devnum")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let num_configs = read_trim(&path, "bNumConfigurations")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let config_value = read_trim(&path, "bConfigurationValue").and_then(|s| s.parse().ok());
        out.push(IPhoneDev {
            serial,
            sysfs: path,
            bus,
            address,
            num_configs,
            config_value,
        });
    }
    let _ = APPLE_VID;
    out
}

fn find_by_serial(serial: &str) -> Option<IPhoneDev> {
    find_iphones().into_iter().find(|d| d.serial == serial)
}

pub fn ensure_carplay_config(serial: &str) -> Result<IPhoneDev, Error> {
    let dev = find_by_serial(serial).ok_or_else(|| Error::NotFound(serial.to_string()))?;

    if dev.num_configs < CP_CONFIG {
        request_carplay_configs(&dev)?;
        await_carplay_configs(serial, || {
            find_by_serial(serial).is_some_and(|d| d.num_configs >= CP_CONFIG)
        })?;
    }

    let dev = find_by_serial(serial).ok_or(Error::Gone("config request"))?;
    if dev.config_value != Some(CP_CONFIG) {
        fs::write(dev.sysfs.join("bConfigurationValue"), CP_CONFIG.to_string())
            .map_err(Error::io(format!("set config {CP_CONFIG}")))?;
    }
    find_by_serial(serial).ok_or(Error::Gone("config select"))
}

pub fn restore_all_default_config() {
    for dev in find_iphones() {
        if dev.config_value == Some(CP_CONFIG) {
            let _ = fs::write(
                dev.sysfs.join("bConfigurationValue"),
                DEFAULT_CONFIG.to_string(),
            );
        }
    }
}

pub fn restore_default_config(serial: &str) {
    if let Some(dev) = find_by_serial(serial) {
        let _ = fs::write(
            dev.sysfs.join("bConfigurationValue"),
            DEFAULT_CONFIG.to_string(),
        );
    }
}

pub fn reset(serial: &str) -> Result<(), Error> {
    let dev = find_by_serial(serial).ok_or_else(|| Error::NotOnBus(serial.to_string()))?;
    open_by_address(dev.bus, dev.address)?
        .reset()
        .wait()
        .map_err(Error::usb("usb reset"))
}

fn request_carplay_configs(dev: &IPhoneDev) -> Result<(), Error> {
    crate::usb_pipe::request_carplay_configs(&open_by_address(dev.bus, dev.address)?)
}

pub fn open(serial: &str) -> Result<nusb::Device, Error> {
    let dev = find_by_serial(serial).ok_or_else(|| Error::NotFound(serial.to_string()))?;
    open_by_address(dev.bus, dev.address)
}

pub fn open_by_address(bus: u8, address: u8) -> Result<nusb::Device, Error> {
    nusb::list_devices()
        .wait()
        .map_err(Error::ListDevices)?
        .find(|d| d.busnum() == bus && d.device_address() == address)
        .ok_or(Error::NoUsbDevice { bus, address })?
        .open()
        .wait()
        .map_err(Error::usb("open usb"))
}
