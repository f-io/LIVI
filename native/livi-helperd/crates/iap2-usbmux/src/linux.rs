use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use nusb::transfer::{ControlIn, ControlType, Recipient};
use nusb::MaybeFuture;

use crate::{APPLE_VID, CP_CONFIG};

const SYSFS_USB: &str = "/sys/bus/usb/devices";
const DEFAULT_CONFIG: u8 = 4;

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
    fs::read_to_string(path.join(name)).ok().map(|s| s.trim().to_string())
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
        let bus = read_trim(&path, "busnum").and_then(|s| s.parse().ok()).unwrap_or(0);
        let address = read_trim(&path, "devnum").and_then(|s| s.parse().ok()).unwrap_or(0);
        let num_configs = read_trim(&path, "bNumConfigurations").and_then(|s| s.parse().ok()).unwrap_or(0);
        let config_value = read_trim(&path, "bConfigurationValue").and_then(|s| s.parse().ok());
        out.push(IPhoneDev { serial, sysfs: path, bus, address, num_configs, config_value });
    }
    let _ = APPLE_VID;
    out
}

fn find_by_serial(serial: &str) -> Option<IPhoneDev> {
    find_iphones().into_iter().find(|d| d.serial == serial)
}

/// Ask the phone to expose its CarPlay configurations (Apple vendor request), then select
/// config 6. Returns the device as seen after the switch.
pub fn ensure_carplay_config(serial: &str) -> Result<IPhoneDev, String> {
    let dev = find_by_serial(serial).ok_or_else(|| format!("iphone {serial} not found"))?;

    if dev.num_configs < CP_CONFIG {
        request_carplay_configs(&dev)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            sleep(Duration::from_millis(200));
            if let Some(d) = find_by_serial(serial)
                && d.num_configs >= CP_CONFIG {
                    break;
                }
            if Instant::now() >= deadline {
                return Err(format!("iphone {} did not expose CarPlay configs", &serial[..8.min(serial.len())]));
            }
        }
    }

    let dev = find_by_serial(serial).ok_or("iphone gone after config request")?;
    if dev.config_value != Some(CP_CONFIG) {
        fs::write(dev.sysfs.join("bConfigurationValue"), CP_CONFIG.to_string())
            .map_err(|e| format!("set config {CP_CONFIG}: {e}"))?;
    }
    find_by_serial(serial).ok_or_else(|| "iphone gone after config select".to_string())
}

/// Hands every attached iPhone back to the system configuration, for shutdown.
pub fn restore_all_default_config() {
    for dev in find_iphones() {
        if dev.config_value == Some(CP_CONFIG) {
            let _ = fs::write(dev.sysfs.join("bConfigurationValue"), DEFAULT_CONFIG.to_string());
        }
    }
}

pub fn restore_default_config(serial: &str) {
    if let Some(dev) = find_by_serial(serial) {
        let _ = fs::write(dev.sysfs.join("bConfigurationValue"), DEFAULT_CONFIG.to_string());
    }
}

// The Apple vendor request that makes the phone re-enumerate with the CarPlay
// configurations. wIndex 4 selects the CarPlay set.
fn request_carplay_configs(dev: &IPhoneDev) -> Result<(), String> {
    let device = open_by_address(dev.bus, dev.address)?;
    device
        .control_in(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: 0x52,
                value: 0x0000,
                index: 0x0004,
                length: 1,
            },
            Duration::from_secs(1),
        )
        .wait()
        .map_err(|e| format!("vendor request 0x52: {e}"))?;
    Ok(())
}

/// Opens the USB device sitting at this bus address.
pub fn open_by_address(bus: u8, address: u8) -> Result<nusb::Device, String> {
    nusb::list_devices()
        .wait()
        .map_err(|e| e.to_string())?
        .find(|d| d.busnum() == bus && d.device_address() == address)
        .ok_or_else(|| format!("usb device {bus}/{address} not found"))?
        .open()
        .wait()
        .map_err(|e| format!("open usb: {e}"))
}
