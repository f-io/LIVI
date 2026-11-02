// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "linux")]
use livi_runtime::bringup::OnCable;
use livi_runtime::livi_sock::Broadcaster;
#[cfg(target_os = "linux")]
use livi_runtime::state::HelperState;

const WATCH_RETRY: Duration = Duration::from_secs(2);

#[cfg(target_os = "linux")]
pub fn drop_dongle_link(mac: String) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = livi_link_host::accessory::drop_link(&mac) {
            eprintln!("[helperd] {mac} stays on the dongle's bluetooth: {e}");
        }
    });
}

#[cfg(target_os = "linux")]
pub fn dongle_on_cable(state: Arc<HelperState>) -> OnCable {
    OnCable(Arc::new(move |mac: &str| {
        let cabled = state.carkit_claims(mac);
        if cabled {
            drop_dongle_link(mac.to_string());
        }
        cabled
    }))
}

pub async fn relay_stations(bcast: Broadcaster) {
    loop {
        let events = bcast.clone();
        let _ = tokio::task::spawn_blocking(move || {
            livi_link_host::ap::watch_stations(|joined, mac| {
                let event = if joined { "joined" } else { "left" };
                println!("[helperd] {mac} {event} the dongle's access point");
                events.push_json(format!(
                    "{{\"type\":\"wifi\",\"event\":\"{event}\",\"mac\":\"{mac}\"}}"
                ));
            })
        })
        .await;
        tokio::time::sleep(WATCH_RETRY).await;
    }
}
