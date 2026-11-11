// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::Error;
use crate::backend::{ensure_carplay_config, restore_default_config};
use crate::mux::MuxHost;
use crate::short;

pub fn socket_path(serial: &str) -> String {
    format!("/tmp/livi-usbmux-{}.sock", short(serial))
}

pub struct MuxDevice {
    pub serial: String,
    pub socket_path: String,
    pub host: Arc<MuxHost>,
}

impl MuxDevice {
    pub async fn start(serial: &str) -> Result<Self, Error> {
        let owned = serial.to_string();
        tokio::task::spawn_blocking(move || ensure_carplay_config(&owned)).await??;
        let host = MuxHost::open(serial).await?;
        Ok(Self {
            serial: serial.to_string(),
            socket_path: socket_path(serial),
            host,
        })
    }
}

impl Drop for MuxDevice {
    fn drop(&mut self) {
        self.host.shutdown();
        restore_default_config(&self.serial);
    }
}

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

    pub async fn ensure(&self, serial: &str) -> Result<Arc<MuxDevice>, Error> {
        if let Some(dev) = self.get(serial) {
            return Ok(dev);
        }
        let dev = Arc::new(MuxDevice::start(serial).await?);
        self.devices
            .lock()
            .unwrap()
            .insert(serial.to_string(), dev.clone());
        Ok(dev)
    }

    pub fn remove(&self, serial: &str) {
        self.devices.lock().unwrap().remove(serial);
    }
}
