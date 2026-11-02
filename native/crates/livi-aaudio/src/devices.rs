// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::OnceLock;

const BUILTIN_SPEAKER: i32 = 2;
const BUILTIN_MIC: i32 = 15;
const HDMI: [i32; 3] = [9, 10, 29];
const HIDDEN: [i32; 3] = [18, 24, 25];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: i32,
    pub kind: i32,
    pub sink: bool,
    pub address: String,
    pub product: String,
}

impl Device {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.address)
    }

    pub fn name(&self) -> String {
        match self.kind {
            BUILTIN_SPEAKER => "Speaker".into(),
            BUILTIN_MIC => "Built-in Mic".into(),
            k if HDMI.contains(&k) => "HDMI".into(),
            _ if self.product.is_empty() => self.key(),
            _ => self.product.clone(),
        }
    }

    pub fn selectable(&self) -> bool {
        !HIDDEN.contains(&self.kind)
    }
}

type Lister = Box<dyn Fn() -> Vec<Device> + Send + Sync>;

static LISTER: OnceLock<Lister> = OnceLock::new();

pub fn set_lister(lister: impl Fn() -> Vec<Device> + Send + Sync + 'static) {
    let _ = LISTER.set(Box::new(lister));
}

pub fn devices() -> Vec<Device> {
    LISTER
        .get()
        .map(|list| list().into_iter().filter(Device::selectable).collect())
        .unwrap_or_default()
}

pub(crate) fn resolve(key: &str, sink: bool) -> Option<i32> {
    devices()
        .into_iter()
        .find(|d| d.sink == sink && d.key() == key)
        .map(|d| d.id)
}
