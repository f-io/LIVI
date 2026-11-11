// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use livi_aaudio::Device;
use livi_core_proto::state::AudioDevice;

use crate::devices::Shared;
use crate::hub::Hub;

const EVERY: Duration = Duration::from_secs(2);

pub async fn follow(hub: Arc<Hub>, _gst: Option<PathBuf>, _shared: Shared) {
    let mut last = None;
    loop {
        let now = tokio::task::spawn_blocking(|| split(livi_aaudio::devices()))
            .await
            .ok();
        if now.is_some() && now != last {
            if let Some((sinks, sources)) = now.clone() {
                hub.update(|s| {
                    s.system.audio_sinks = sinks;
                    s.system.audio_sources = sources;
                });
            }
            last = now;
        }
        tokio::time::sleep(EVERY).await;
    }
}

fn split(devices: Vec<Device>) -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let shown = |d: &Device| AudioDevice {
        id: d.key(),
        name: d.name(),
        is_default: false,
        offline: false,
    };
    let sinks = devices.iter().filter(|d| d.sink).map(shown).collect();
    let sources = devices.iter().filter(|d| !d.sink).map(shown).collect();
    (sinks, sources)
}
