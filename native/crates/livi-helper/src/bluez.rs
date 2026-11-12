// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use livi_core_proto::helper::HelperSockets;
use livi_helper_stack::bringup::CpConfig;
use livi_helper_stack::bt;
use livi_helper_stack::driver::spawn_link;
use livi_helper_stack::ident::Identity;
use livi_helper_stack::livi_sock::{Bluez, Broadcaster, run_bluetooth};
use livi_helper_stack::mfi_async::SharedCoprocessor;
use livi_helper_stack::net;
use livi_helper_stack::reconnect;
use livi_helper_stack::sco::ScoSink;
use livi_helper_stack::state::HelperState;
use livi_wifi::Security;
use tokio::sync::{mpsc, watch};

use crate::aa::{AaConfig, Told, WiredPhones, bootstrap, on_dongle, stream};

const AP_IP: &str = "10.10.0.1";

pub struct Bluetooth {
    pub adapter: String,
    pub wireless: bt::Wireless,
    pub sockets: HelperSockets,
    pub auth: SharedCoprocessor,
    pub identity: Identity,
    pub cp: CpConfig,
    pub dongle_ap: bool,
    pub bcast: Broadcaster,
    pub state: Arc<HelperState>,
    pub bluez: Arc<watch::Sender<Option<Bluez>>>,
    pub aa_events: Broadcaster,
    pub wired_phones: WiredPhones,
    pub sco_sink: ScoSink,
}

pub fn ap_iface(chosen: &str) -> String {
    let iface = crate::or(chosen, "wlan0");
    if iface != livi_link_host::link::CHOICE {
        return iface;
    }
    livi_link_host::link::host_iface().unwrap_or(iface)
}

pub fn session_cp(cp: &CpConfig, dongle_ap: bool) -> CpConfig {
    if !dongle_ap {
        return cp.clone();
    }
    CpConfig {
        ap_mac: livi_link_host::ap::mac().or_else(|| cp.ap_mac.clone()),
        ..cp.clone()
    }
}

async fn resolve_cp(cp: &CpConfig, dongle_ap: bool) -> CpConfig {
    let base = cp.clone();
    tokio::task::spawn_blocking(move || session_cp(&base, dongle_ap))
        .await
        .unwrap_or_else(|_| cp.clone())
}

async fn adapter(chosen: &str) -> String {
    let adapter = crate::or(chosen, "hci0");
    if adapter != livi_link_host::link::CHOICE {
        return adapter;
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let ours = Arc::new(AtomicBool::new(false));
    let lost = ours.clone();
    livi_link_host::bt::attach(
        move |index| {
            ours.store(true, Ordering::Relaxed);
            if tx.send(index).is_err() {
                eprintln!("[bt] the dongle's controller came back, starting over with it");
                std::process::exit(1);
            }
        },
        move || {
            if lost.load(Ordering::Relaxed) {
                eprintln!("[bt] the dongle's controller is gone, starting over");
                std::process::exit(1);
            }
        },
    );
    match rx.recv().await {
        Some(index) => format!("hci{index}"),
        None => {
            eprintln!("[bt] the dongle's controller is out of reach, starting over");
            std::process::exit(1);
        }
    }
}

pub async fn serve(b: Bluetooth, mut stop: watch::Receiver<bool>) -> Result<(), Box<dyn Error>> {
    let wireless = b.wireless;
    let adapter = adapter(&b.adapter).await;
    let _ =
        tokio::task::spawn_blocking(move || livi_helper_stack::bluetoothd::setup(wireless)).await;
    println!(
        "[helper] starting BlueZ profile on {adapter} (wireless CarPlay {}, wireless Android Auto {})",
        if wireless.carplay { "on" } else { "off" },
        if wireless.android_auto { "on" } else { "off" }
    );
    let (conn, mut incoming, held) = bt::start(&adapter, &b.identity.name, wireless).await?;
    let bt_mac = bt::adapter_address(&conn, &adapter).await?;
    println!(
        "[helper] adapter {} up (RFCOMM ch {})",
        livi_base::hex::mac(&bt_mac),
        bt::IAP_CHANNEL
    );
    let identity = Identity {
        bt_mac,
        ..b.identity.clone()
    };
    let _ = b.bluez.send(Some(Bluez {
        bus: conn.clone(),
        adapter: adapter.clone(),
        bt_mac,
    }));

    tokio::spawn(reconnect::run(
        reconnect::Bluez {
            conn: conn.clone(),
            adapter: adapter.clone(),
            ap_iface: b.cp.wifi_iface.clone(),
        },
        b.state.clone(),
    ));

    let hfp = livi_helper_stack::hfp::Hfp::default();
    hfp.set_events(b.aa_events.clone());
    if let Err(e) = bt::start_hfp(&conn, &adapter, hfp).await {
        eprintln!("[hfp] profile registration failed: {e}");
    }
    livi_helper_stack::sco::serve(
        b.sockets.sco_mic.clone(),
        b.aa_events.clone(),
        b.sco_sink.clone(),
    );

    if wireless.android_auto {
        crate::aa::serve_wifi(
            livi_aa::consts::TCP_PORT,
            b.sockets.sessions.clone(),
            b.aa_events.clone(),
        );
        let android_auto = bt::start_aa(&conn, &adapter)
            .await
            .map_err(|e| e.to_string());
        match android_auto {
            Ok(incoming) => {
                let cfg = AaConfig {
                    ssid: b.cp.ssid.clone(),
                    passphrase: b.cp.passphrase.clone(),
                    channel: b.cp.channel,
                    wifi_iface: b.cp.wifi_iface.clone(),
                    ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| AP_IP.into()),
                    port: livi_aa::consts::TCP_PORT,
                };
                if let Err(e) = bt::start_ble_ad(&conn, &adapter, &identity.name).await {
                    eprintln!("[aa] BLE advertisement failed: {e}");
                }
                tokio::spawn(watch_android_auto(
                    incoming,
                    cfg,
                    b.aa_events.clone(),
                    b.wired_phones.clone(),
                    b.state.clone(),
                    b.dongle_ap,
                ));
            }
            Err(e) => eprintln!("[aa] profile registration failed: {e}"),
        }
    }

    let shared_events = Broadcaster::default();
    let mpris = match bt::start_media_player(&conn, &adapter, shared_events.clone()).await {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("[bt] media player failed: {e}");
            None
        }
    };
    {
        let bus = conn.clone();
        let deps = livi_helper_stack::shared_sock::SharedSockDeps {
            adapter: adapter.clone(),
            wifi_iface: b.cp.wifi_iface.clone(),
            events: shared_events,
            set_playback_status: Box::new(move |state| {
                let Some(h) = mpris.clone() else { return };
                let status = match state {
                    "playing" => "Playing",
                    "paused" => "Paused",
                    _ => "Stopped",
                };
                tokio::spawn(async move { h.set_status(status).await });
            }),
            deauth_dongle: b
                .dongle_ap
                .then_some(livi_link_host::ap::deauth as fn() -> Option<usize>),
            bluetooth: None,
        };
        let path = b.sockets.shared.clone();
        tokio::spawn(async move {
            if let Err(e) = livi_helper_stack::shared_sock::serve(&path, Some(bus), deps).await {
                eprintln!("[shared-sock] ended: {e}");
            }
        });
    }

    let bus = conn.clone();
    loop {
        tokio::select! {
            _ = stop.changed() => {
                bt::let_go(&conn, &adapter, &held).await;
                return Ok(());
            }
            conn = incoming.recv(), if wireless.carplay => {
                let Some(conn) = conn else { return Ok(()) };
                let cp = CpConfig { on_cable: Some(bt::on_cable(b.state.clone(), &bus, &adapter)), ..resolve_cp(&b.cp, b.dongle_ap).await };
                run_bluetooth(|cfg| spawn_link(conn.fd, cfg, false), b.auth.clone(), identity.clone(), cp, b.bcast.clone(), b.state.clone(), conn.peer_mac);
            }
        }
    }
}

async fn access_point(cfg: &AaConfig, dongle_ap: bool) -> Told {
    let base = cfg.clone();
    tokio::task::spawn_blocking(move || {
        if dongle_ap {
            let passphrase = base.passphrase.clone();
            return on_dongle(&base, passphrase);
        }
        let iface = base.wifi_iface.clone();
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
            ..base
        };
        (told, security, bssid)
    })
    .await
    .unwrap_or_else(|_| (cfg.clone(), Security::default(), String::new()))
}

async fn watch_android_auto(
    mut incoming: mpsc::UnboundedReceiver<bt::IncomingConn>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    state: Arc<HelperState>,
    dongle_ap: bool,
) {
    while let Some(conn) = incoming.recv().await {
        println!("[aa] phone connected mac={}", conn.peer_mac);
        let (cfg, bcast, wired, state) = (cfg.clone(), bcast.clone(), wired.clone(), state.clone());
        tokio::spawn(async move {
            let mac = conn.peer_mac;
            match stream(conn.fd) {
                Ok(sock) => {
                    let told = access_point(&cfg, dongle_ap).await;
                    bootstrap(sock, &mac, told, &bcast, &wired, &state).await;
                }
                Err(e) => eprintln!("[aa] {mac}: {e}"),
            }
        });
    }
}
