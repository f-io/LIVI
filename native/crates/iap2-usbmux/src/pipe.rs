// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

pub trait MuxWriter: Send {
    fn write(&mut self, data: &[u8]) -> Result<(), String>;
}

pub trait MuxReader: Send {
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, String>;
}

pub type MuxPipes = (Box<dyn MuxWriter>, Box<dyn MuxReader>);

#[derive(Debug, Clone)]
pub struct PhoneInfo {
    pub serial: String,
    pub num_configs: u8,
    pub config_value: Option<u8>,
}
