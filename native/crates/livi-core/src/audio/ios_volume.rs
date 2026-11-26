// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::OnceLock;

pub trait IosVolume: Send + Sync {
    fn get(&self) -> Option<f64>;
    fn set(&self, level: f64);
}

static PROVIDER: OnceLock<Box<dyn IosVolume>> = OnceLock::new();

pub fn register(provider: impl IosVolume + 'static) {
    let _ = PROVIDER.set(Box::new(provider));
}

pub(crate) fn get() -> Option<f64> {
    PROVIDER.get().and_then(|p| p.get())
}

pub(crate) fn set(level: f64) {
    if let Some(p) = PROVIDER.get() {
        p.set(level);
    }
}
