// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;

pub use livi_helper::aa::{AaConfig, WiredPhones};
use livi_helper::aa::{Told, bootstrap, on_dongle, stream};
use livi_runtime::livi_sock::Broadcaster;
use livi_runtime::net;
use livi_wifi::Security;

async fn access_point(cfg: &AaConfig) -> Told {
    let base = cfg.clone();
    tokio::task::spawn_blocking(move || {
        let dc = crate::linux_main::DeviceConfig::load();
        let iface = dc.string("wifiInterface", "LIVI_WIFI_IFACE", &base.wifi_iface);
        let passphrase = dc.string("wifiPassword", "LIVI_PASSPHRASE", &base.passphrase);
        if iface == livi_link_host::link::CHOICE {
            return on_dongle(&base, passphrase);
        }
        let live = livi_link_host::on_air(&iface);
        let ssid = live
            .as_ref()
            .map(|ap| ap.ssid.clone())
            .filter(|s| !s.is_empty());
        let channel = live.as_ref().map_or(base.channel, |ap| ap.channel);
        let security = live.map(|ap| ap.security).unwrap_or_default();
        let bssid = net::wlan_mac(&iface).unwrap_or_default();
        let told = AaConfig {
            ssid: ssid.unwrap_or_else(|| base.ssid.clone()),
            channel,
            wifi_iface: iface,
            passphrase,
            ..base
        };
        (told, security, bssid)
    })
    .await
    .unwrap_or_else(|_| (cfg.clone(), Security::default(), String::new()))
}

pub async fn watch(
    mut incoming: tokio::sync::mpsc::UnboundedReceiver<livi_runtime::bt::IncomingConn>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    state: Arc<livi_runtime::state::HelperState>,
) {
    while let Some(conn) = incoming.recv().await {
        println!("[aa] phone connected mac={}", conn.peer_mac);
        let (cfg, bcast, wired, state) = (cfg.clone(), bcast.clone(), wired.clone(), state.clone());
        tokio::spawn(async move {
            let mac = conn.peer_mac;
            match stream(conn.fd) {
                Ok(sock) => {
                    let told = access_point(&cfg).await;
                    bootstrap(sock, &mac, told, &bcast, &wired, &state).await;
                }
                Err(e) => eprintln!("[aa] {mac}: {e}"),
            }
        });
    }
}
