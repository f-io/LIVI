// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::server::Core;

const BACKLIGHTS: &str = "/sys/class/backlight";

struct Backlight {
    brightness: PathBuf,
    max: u32,
}

fn backlights() -> Vec<Backlight> {
    let Ok(dir) = std::fs::read_dir(BACKLIGHTS) else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| {
            let max = std::fs::read_to_string(e.path().join("max_brightness")).ok()?;
            Some(Backlight {
                brightness: e.path().join("brightness"),
                max: max.trim().parse().ok().filter(|&m| m > 0)?,
            })
        })
        .collect()
}

pub async fn follow(core: Arc<Core>) {
    let lights = backlights();
    if lights.is_empty() {
        return;
    }
    let mut state = core.hub.watch();
    let mut applied: Option<f64> = None;
    loop {
        let level = {
            let s = state.borrow_and_update();
            let dimmer = s
                .config
                .display_brightness_auto
                .then(|| s.telemetry.get("dimmerPct").and_then(Value::as_f64))
                .flatten()
                .map(|pct| pct / 100.0);
            dimmer.unwrap_or(s.config.display_brightness)
        };
        if level.is_finite() && applied != Some(level) {
            applied = Some(level);
            for l in &lights {
                // No brightness setting leaves the display dark.
                let raw = ((level.clamp(0.0, 1.0) * l.max as f64).round() as u32).max(1);
                if let Err(e) = std::fs::write(&l.brightness, raw.to_string()) {
                    eprintln!("[backlight] {}: {e}", l.brightness.display());
                }
            }
        }
        if state.changed().await.is_err() {
            return;
        }
    }
}
