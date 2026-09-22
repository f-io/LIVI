// One usbmux instance per attached iPhone, each with its own socket.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::{ensure_carplay_config, find_iphones, restore_default_config};
use crate::mux::MuxHost;

pub fn socket_path(serial: &str) -> String {
    format!("/tmp/livi-usbmux-{}.sock", &serial[..8.min(serial.len())])
}

pub struct MuxDevice {
    pub serial: String,
    pub socket_path: String,
    pub host: Arc<MuxHost>,
}

impl MuxDevice {
    pub fn start(serial: &str) -> Result<Self, String> {
        ensure_carplay_config(serial)?;
        let host = MuxHost::open(serial)?;
        Ok(Self { serial: serial.to_string(), socket_path: socket_path(serial), host })
    }
}

impl Drop for MuxDevice {
    fn drop(&mut self) {
        restore_default_config(&self.serial);
    }
}

/// Tracks one MuxDevice per serial, adding phones as they appear and dropping them when
/// they are unplugged.
#[derive(Default)]
pub struct MuxRegistry {
    devices: Mutex<HashMap<String, Arc<MuxDevice>>>,
}

impl MuxRegistry {
    pub fn get(&self, serial: &str) -> Option<Arc<MuxDevice>> {
        self.devices.lock().unwrap().get(serial).cloned()
    }

    pub fn serials(&self) -> Vec<String> {
        self.devices.lock().unwrap().keys().cloned().collect()
    }

    pub fn ensure(&self, serial: &str) -> Result<Arc<MuxDevice>, String> {
        if let Some(dev) = self.get(serial) {
            return Ok(dev);
        }
        let dev = Arc::new(MuxDevice::start(serial)?);
        self.devices.lock().unwrap().insert(serial.to_string(), dev.clone());
        Ok(dev)
    }

    pub fn remove(&self, serial: &str) {
        self.devices.lock().unwrap().remove(serial);
    }

    /// Brings up every attached iPhone that is not tracked yet and drops the ones that went
    /// away. Returns (added, removed) serials.
    pub fn sync(&self) -> (Vec<String>, Vec<String>) {
        let present: Vec<String> = find_iphones().into_iter().map(|d| d.serial).collect();
        let known = self.serials();

        let mut added = Vec::new();
        for serial in &present {
            if !known.contains(serial) {
                match self.ensure(serial) {
                    Ok(_) => added.push(serial.clone()),
                    Err(e) => eprintln!("[muxd] {}: {e}", &serial[..8.min(serial.len())]),
                }
            }
        }

        let mut removed = Vec::new();
        for serial in known {
            if !present.contains(&serial) {
                self.remove(&serial);
                removed.push(serial);
            }
        }
        (added, removed)
    }
}
