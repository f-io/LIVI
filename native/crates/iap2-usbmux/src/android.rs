// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Mutex;
use std::time::Duration;

use nusb::descriptors::language_id::US_ENGLISH;
use nusb::{Device, DeviceId, MaybeFuture};

use crate::pipe::PhoneInfo;
use crate::usb_pipe::{await_carplay_configs, request_carplay_configs};
use crate::{APPLE_VID, CP_CONFIG, DEFAULT_CONFIG};

const STRING_TIMEOUT: Duration = Duration::from_secs(1);

struct Phone {
    id: DeviceId,
    serial: String,
    device: Device,
}

struct Bus {
    phones: Vec<Phone>,
    refused: Vec<DeviceId>,
}

static BUS: Mutex<Bus> = Mutex::new(Bus {
    phones: Vec::new(),
    refused: Vec::new(),
});

fn serial_of(device: &Device) -> Option<String> {
    let index = device.device_descriptor().serial_number_string_index()?;
    device
        .get_string_descriptor(index, US_ENGLISH, STRING_TIMEOUT)
        .wait()
        .ok()
        .map(|s| s.trim_end_matches('\0').to_string())
        .filter(|s| !s.is_empty())
}

fn phones() -> Vec<(String, Device)> {
    let apple: Vec<_> = match nusb::list_devices().wait() {
        Ok(list) => list.filter(|d| d.vendor_id() == APPLE_VID).collect(),
        Err(e) => {
            eprintln!("[usbmux] list devices: {e}");
            Vec::new()
        }
    };
    let mut bus = BUS.lock().unwrap();
    bus.phones.retain(|p| apple.iter().any(|d| d.id() == p.id));
    bus.refused.retain(|id| apple.iter().any(|d| d.id() == *id));
    for info in apple {
        let id = info.id();
        if bus.phones.iter().any(|p| p.id == id) || bus.refused.contains(&id) {
            continue;
        }
        match info.open().wait() {
            Ok(device) => match serial_of(&device) {
                Some(serial) => bus.phones.push(Phone { id, serial, device }),
                None => bus.refused.push(id),
            },
            Err(e) => {
                eprintln!("[usbmux] {:?}: {e}", info.product_string());
                bus.refused.push(id);
            }
        }
    }
    bus.phones
        .iter()
        .map(|p| (p.serial.clone(), p.device.clone()))
        .collect()
}

fn info(serial: String, device: &Device) -> PhoneInfo {
    PhoneInfo {
        serial,
        num_configs: device.device_descriptor().num_configurations(),
        config_value: device
            .active_configuration()
            .ok()
            .map(|c| c.configuration_value()),
    }
}

pub fn open(serial: &str) -> Result<Device, String> {
    phones()
        .into_iter()
        .find(|(s, _)| s == serial)
        .map(|(_, device)| device)
        .ok_or_else(|| format!("iphone {serial} not found"))
}

pub fn find_iphones() -> Vec<PhoneInfo> {
    phones()
        .into_iter()
        .map(|(serial, device)| info(serial, &device))
        .collect()
}

pub fn ensure_carplay_config(serial: &str) -> Result<PhoneInfo, String> {
    let device = open(serial)?;
    if device.device_descriptor().num_configurations() < CP_CONFIG {
        request_carplay_configs(&device)?;
        drop(device);
        await_carplay_configs(serial, || {
            open(serial).is_ok_and(|d| d.device_descriptor().num_configurations() >= CP_CONFIG)
        })?;
    }
    let device = open(serial)?;
    if info(serial.to_string(), &device).config_value != Some(CP_CONFIG) {
        device
            .set_configuration(CP_CONFIG)
            .wait()
            .map_err(|e| format!("set config {CP_CONFIG}: {e}"))?;
    }
    Ok(info(serial.to_string(), &device))
}

pub fn restore_default_config(serial: &str) {
    if let Ok(device) = open(serial)
        && let Err(e) = device.set_configuration(DEFAULT_CONFIG).wait()
    {
        eprintln!("[usbmux] set config {DEFAULT_CONFIG}: {e}");
    }
}

pub fn reset(serial: &str) -> Result<(), String> {
    open(serial)?
        .reset()
        .wait()
        .map_err(|e| format!("usb reset: {e}"))
}
