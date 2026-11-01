// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use iap2_link::LinkConfig;
#[cfg(target_os = "linux")]
use iap2_usbmux::{MuxRegistry, find_iphones};
#[cfg(target_os = "linux")]
use iap2_wired::open_carkit;
use livi_runtime::bringup::{CpConfig, run_accessory};
use livi_runtime::driver::spawn_link_stream;
use livi_runtime::ident::Identity;
use livi_runtime::livi_sock::{Broadcaster, SharedTag, pump_artwork, pump_events_for};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::state::HelperState;

use crate::link::LinkPresence;

fn announce_gone(bcast: &Broadcaster, serial: &str) {
    bcast.push_json(format!(
        "{{\"type\":\"device-gone\",\"src\":\"carkit\",\"usbUdid\":\"{serial}\"}}"
    ));
}

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const MFI_RETRY: Duration = Duration::from_secs(1);
#[cfg(target_os = "linux")]
const GIVE_UP_ATTEMPTS: u32 = 3;

#[cfg(target_os = "linux")]
pub async fn watch(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    dongle: Dongle,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    link: Arc<LinkPresence>,
) {
    let registry = Arc::new(MuxRegistry::default());
    let mut failed: HashMap<String, u32> = HashMap::new();
    let mut cancels: HashMap<String, Arc<Notify>> = HashMap::new();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = link.changed().notified() => {}
        }

        let present: Vec<String> = find_iphones().into_iter().map(|d| d.serial).collect();
        failed.retain(|serial, _| present.contains(serial));
        for serial in registry.serials() {
            if !present.contains(&serial) {
                println!("[wired] {} unplugged", short(&serial));
                if let Some(c) = cancels.remove(&serial) {
                    c.notify_one();
                }
                announce_gone(&bcast, &serial);
                registry.remove(&serial);
            }
        }
        if !link.is_present() {
            continue;
        }

        let active = registry.serials();
        for serial in present {
            if active.contains(&serial) {
                continue;
            }
            if failed.get(&serial).is_some_and(|n| *n >= GIVE_UP_ATTEMPTS) {
                continue;
            }
            let dev = match registry.ensure(&serial) {
                Ok(dev) => {
                    failed.remove(&serial);
                    dev
                }
                Err(e) => {
                    let n = failed.entry(serial.clone()).or_insert(0);
                    *n += 1;
                    if *n >= GIVE_UP_ATTEMPTS {
                        eprintln!(
                            "[wired] {}: giving up after {n} attempts ({e}) — ignored until replug",
                            short(&serial)
                        );
                    } else {
                        eprintln!("[wired] {}: usbmux failed: {e}", short(&serial));
                    }
                    continue;
                }
            };
            println!("[wired] {}: usbmux up, opening carkit", short(&serial));

            let ctx = WiredCtx {
                auth: auth.clone(),
                identity: identity.clone(),
                cp: cp.clone(),
                dongle,
                bcast: bcast.clone(),
                state: state.clone(),
                link: link.clone(),
            };
            let cancel = Arc::new(Notify::new());
            cancels.insert(serial.clone(), cancel.clone());
            let registry = registry.clone();
            tokio::spawn(async move {
                let ncm = start_ncm_bridge(&dev.serial);
                let serial = dev.serial.clone();
                match open_carkit(&dev).await {
                    Ok(channel) => match channel.into_stream() {
                        Some(stream) => {
                            println!(
                                "[wired] {}: carkit channel up, starting iAP2",
                                short(&serial)
                            );
                            run_wired_session(serial.clone(), stream, ncm, ctx, cancel).await;
                        }
                        None => {
                            eprintln!("[wired] {}: carkit stream unavailable", short(&serial));
                            drop(ncm);
                        }
                    },
                    Err(e) => {
                        eprintln!("[wired] {}: carkit failed: {e}", short(&serial));
                        drop(ncm);
                    }
                }
                registry.remove(&serial);
            });
        }
    }
}

#[cfg(target_os = "macos")]
pub async fn watch_usbmuxd(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    dongle: Dongle,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    link: Arc<LinkPresence>,
) {
    use std::collections::HashSet;

    let mut active: HashMap<String, Arc<Notify>> = HashMap::new();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = link.changed().notified() => {}
        }

        let devices = match iap2_wired::usbmuxd::devices().await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let present: HashSet<String> = devices.iter().map(|d| d.udid.clone()).collect();
        active.retain(|udid, cancel| {
            if state.take_redo(udid) {
                return false;
            }
            let keep = present.contains(udid);
            if !keep {
                println!("[wired] {} unplugged", short(udid));
                cancel.notify_one();
                announce_gone(&bcast, udid);
            }
            keep
        });
        if !link.is_present() {
            continue;
        }

        for device in devices {
            if active.contains_key(&device.udid) {
                continue;
            }
            let cancel = Arc::new(Notify::new());
            active.insert(device.udid.clone(), cancel.clone());
            let (found, late) = tokio::sync::watch::channel(None);
            let mut ctx = WiredCtx {
                auth: auth.clone(),
                identity: identity.clone(),
                cp: cp.clone(),
                dongle,
                bcast: bcast.clone(),
                state: state.clone(),
                link: link.clone(),
            };
            ctx.cp.av_iface_late = Some(late);
            tokio::spawn(async move {
                match iap2_wired::usbmuxd::open(&device).await {
                    Ok(stream) => {
                        println!(
                            "[wired] {}: usbmuxd carkit up, starting iAP2",
                            short(&device.udid)
                        );
                        let finder = tokio::spawn(find_usb_iface(device.udid.clone(), found));
                        let ncm = LocalNcm::System(None);
                        run_wired_session(device.udid.clone(), stream, ncm, ctx, cancel).await;
                        finder.abort();
                    }
                    Err(e) => {
                        eprintln!(
                            "[wired] {}: usbmuxd carkit failed: {e}",
                            short(&device.udid)
                        )
                    }
                }
            });
        }
    }
}

#[cfg(target_os = "macos")]
const USB_IFACE_TRIES: u32 = 20;
#[cfg(target_os = "macos")]
const USB_IFACE_POLL: Duration = Duration::from_millis(500);

#[cfg(target_os = "macos")]
async fn find_usb_iface(udid: String, found: tokio::sync::watch::Sender<Option<String>>) {
    let mut last = String::new();
    for _ in 0..USB_IFACE_TRIES {
        match iap2_wired::mac_network::discover(&udid).await {
            Ok(iface) => {
                println!("[wired] {}: USB network interface {iface}", short(&udid));
                let _ = found.send(Some(iface));
                return;
            }
            Err(e) => last = e,
        }
        tokio::time::sleep(USB_IFACE_POLL).await;
    }
    eprintln!("[wired] {}: no USB network interface: {last}", short(&udid));
}

#[derive(Clone, Copy, Default)]
pub struct Dongle {
    pub ap_mac: Option<fn() -> Option<String>>,
    pub bt_mac: Option<fn() -> Option<[u8; 6]>>,
}

#[derive(Clone)]
struct WiredCtx {
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    dongle: Dongle,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    link: Arc<LinkPresence>,
}

async fn run_wired_session<S>(
    serial: String,
    stream: S,
    ncm: LocalNcm,
    ctx: WiredCtx,
    cancel: Arc<Notify>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut cp = ctx.cp;
    if let Some(name) = ncm.ifname() {
        cp.av_iface = Some(name.to_string());
    }
    if let Some(read) = ctx.dongle.ap_mac
        && let Ok(Some(mac)) = tokio::task::spawn_blocking(read).await
    {
        cp.ap_mac = Some(mac);
    }
    let mut identity = ctx.identity;
    if let Some(read) = ctx.dongle.bt_mac
        && let Ok(Some(mac)) = tokio::task::spawn_blocking(read).await
    {
        identity.bt_mac = mac;
    }
    let link = LinkConfig {
        max_outgoing: 4,
        control_version: 2,
        zero_ack: true,
        ..LinkConfig::default()
    };
    let (ch, art_rx) = spawn_link_stream(stream, link, true);
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let ident: SharedTag = Default::default();
    ctx.state.carkit_started(ident.clone());
    let restart = Arc::new(Notify::new());
    let (asked, again) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let reset = ctx
        .state
        .wired_started(&serial, restart.clone(), asked.clone());
    cp.start_again = Some(again.clone());
    let forward = tokio::spawn(forward_starts(
        short(&serial).to_string(),
        asked,
        again,
        ctx.link.clone(),
        ctx.auth.clone(),
    ));
    tokio::spawn(pump_events_for(
        rx,
        ctx.bcast.clone(),
        "wired",
        Some(serial.clone()),
        ident.clone(),
    ));
    tokio::spawn(pump_artwork(art_rx, ctx.bcast, ident.clone()));
    let mut usb_reset = false;
    tokio::select! {
        _ = run_accessory(ch, ctx.auth, identity, cp, tx, ctx.state.vehicle_feed()) => {}
        _ = cancel.notified() => println!("[wired] {}: session cancelled on unplug", short(&serial)),
        _ = restart.notified() => println!("[wired] {}: session ended for a fresh start", short(&serial)),
        _ = reset.notified() => {
            println!("[wired] {}: no CarPlay session came up, resetting the phone's USB", short(&serial));
            usb_reset = true;
        }
    }
    forward.abort();
    ctx.state.wired_ended(&serial);
    ctx.state.carkit_ended(&ident);
    drop(ncm);
    if usb_reset {
        let target = serial.clone();
        match tokio::task::spawn_blocking(move || iap2_usbmux::reset(&target)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("[wired] {}: {e}", short(&serial)),
            Err(e) => eprintln!("[wired] {}: usb reset task: {e}", short(&serial)),
        }
    }
}

async fn forward_starts(
    serial: String,
    asked: Arc<Notify>,
    again: Arc<Notify>,
    link: Arc<LinkPresence>,
    mut auth: SharedCoprocessor,
) {
    use livi_runtime::AsyncAuth;
    loop {
        asked.notified().await;
        if !link.is_present() {
            println!("[wired] {serial}: another start waits for the MFi chip");
        }
        loop {
            link.wait_until(true).await;
            if auth.protocol_major().await.is_ok() {
                break;
            }
            tokio::time::sleep(MFI_RETRY).await;
        }
        again.notify_one();
    }
}

fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}

enum LocalNcm {
    #[cfg(target_os = "linux")]
    Local(iap2_usbmux::NcmBridge),
    System(Option<String>),
}

impl LocalNcm {
    fn ifname(&self) -> Option<&str> {
        match self {
            #[cfg(target_os = "linux")]
            LocalNcm::Local(b) => Some(b.ifname.as_str()),
            LocalNcm::System(name) => name.as_deref(),
        }
    }
}

#[cfg(target_os = "linux")]
fn start_ncm_bridge(serial: &str) -> LocalNcm {
    match iap2_usbmux::NcmBridge::start(serial) {
        Ok(b) => LocalNcm::Local(b),
        Err(e) => {
            eprintln!("[wired] {}: ncm bridge unavailable: {e}", short(serial));
            LocalNcm::System(None)
        }
    }
}
