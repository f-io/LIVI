// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::server::Core;

#[cfg(target_os = "macos")]
mod coreaudio;
#[cfg(target_os = "linux")]
mod pactl;

#[cfg(target_os = "macos")]
use coreaudio as output;
#[cfg(target_os = "linux")]
use pactl as output;

const ECHO_WINDOW: Duration = Duration::from_millis(400);
const READ_DEBOUNCE: Duration = Duration::from_millis(120);
const SAME: f64 = 0.005;

pub enum Fault {
    Missing,
    Other(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Missing => f.write_str("not there"),
            Fault::Other(e) => f.write_str(e),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputEvent {
    Changed,
    New,
}

fn configured(device: Option<&str>) -> &str {
    device.map(str::trim).unwrap_or("")
}

#[derive(Default)]
struct Mixer {
    missing: Option<String>,
    wrote_at: Option<Instant>,
}

impl Mixer {
    fn fall_back(&mut self, device: &str) {
        if self.missing.as_deref() != Some(device) {
            eprintln!("[volume] {device} is not there, the default output is followed until it is");
            self.missing = Some(device.to_string());
        }
    }

    async fn get(&mut self, device: Option<&str>) -> Option<f64> {
        let device = configured(device);
        match output::get(device).await {
            Ok(level) => {
                self.missing = None;
                Some(level)
            }
            Err(Fault::Missing) if !device.is_empty() => {
                self.fall_back(device);
                output::get("").await.ok()
            }
            Err(_) => None,
        }
    }

    async fn set(&mut self, level: f64, device: Option<&str>) {
        let device = configured(device);
        let pct = (level.clamp(0.0, 1.0) * 100.0).round() as u32;
        self.wrote_at = Some(Instant::now());
        let mut res = output::set(device, level).await;
        match &res {
            Err(Fault::Missing) if !device.is_empty() => {
                self.fall_back(device);
                res = output::set("", level).await;
            }
            Ok(()) => self.missing = None,
            Err(_) => {}
        }
        self.wrote_at = Some(Instant::now());
        match res {
            Ok(()) => println!("[volume] system volume {pct} %"),
            Err(e) => {
                let name = if device.is_empty() {
                    "the default output"
                } else {
                    device
                };
                eprintln!("[volume] {name} not set to {pct} %: {e}");
            }
        }
    }

    fn own_echo(&self) -> bool {
        self.wrote_at.is_some_and(|at| at.elapsed() < ECHO_WINDOW)
    }
}

async fn take_system_level(core: &Core, level: f64, how: &str) {
    if (level - core.hub.config().hu_volume).abs() < SAME {
        return;
    }
    println!("[volume] head unit {how}, {} %", (level * 100.0).round());
    if let Err(e) = core.set_config(&json!({ "huVolume": level })).await {
        eprintln!("[volume] head unit level not saved: {e}");
    }
}

pub async fn follow(core: Arc<Core>) {
    let mut state = core.hub.watch();
    let mut mixer = Mixer::default();
    let mut following: Option<Option<String>> = None;
    let mut applied: Option<f64> = None;
    let mut watcher: Option<output::Watch> = None;
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut read_at: Option<Instant> = None;
    let mut last: Option<f64> = None;
    loop {
        let cfg = state.borrow_and_update().config.clone();
        if !cfg.hu_volume_link_system {
            if following.take().is_some() {
                watcher = None;
                println!("[volume] the system volume goes its own way");
            }
        } else if following.as_ref() != Some(&cfg.audio_output_device) {
            following = Some(cfg.audio_output_device.clone());
            if watcher.is_none() {
                watcher = Some(output::watch(tx.clone()));
                println!("[volume] watching the output for changes made outside LIVI");
            }
            let level = mixer.get(cfg.audio_output_device.as_deref()).await;
            if let Some(level) = level {
                take_system_level(&core, level, "starts from the system").await;
            }
            applied = Some(level.unwrap_or(cfg.hu_volume));
            last = level;
        } else if applied.is_none_or(|a| (cfg.hu_volume - a).abs() >= SAME) {
            applied = Some(cfg.hu_volume);
            mixer
                .set(cfg.hu_volume, cfg.audio_output_device.as_deref())
                .await;
        }
        let deadline = read_at;
        let debounce = async move {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            Some(event) = events.recv() => match event {
                OutputEvent::Changed => read_at = Some(Instant::now() + READ_DEBOUNCE),
                OutputEvent::New => {
                    let Some(device) = mixer.missing.clone() else { continue };
                    if let Ok(level) = output::get(&device).await {
                        println!("[volume] {device} is back");
                        mixer.missing = None;
                        applied = Some(level);
                        last = Some(level);
                        take_system_level(&core, level, "starts from the system").await;
                    }
                }
            },
            () = debounce => {
                read_at = None;
                if mixer.own_echo() || watcher.is_none() {
                    continue;
                }
                let Some(level) = mixer.get(cfg.audio_output_device.as_deref()).await else {
                    continue;
                };
                if last.is_some_and(|l| (level - l).abs() < SAME) {
                    continue;
                }
                last = Some(level);
                applied = Some(level);
                take_system_level(&core, level, "follows the system").await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_configured_output_is_the_default_one() {
        assert_eq!(configured(None), "");
        assert_eq!(configured(Some("  ")), "");
        assert_eq!(
            configured(Some(" BuiltInSpeakerDevice ")),
            "BuiltInSpeakerDevice"
        );
    }
}
