// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use iap2_link::LinkConfig;
#[cfg(any(target_os = "linux", target_os = "android"))]
use iap2_usbmux::{MuxRegistry, find_iphones};
#[cfg(any(target_os = "linux", target_os = "android"))]
use iap2_wired::open_carkit;
use livi_core_proto::helper_events::{DeviceSource, HelperEvent, HelperKind};
use livi_helper_stack::bringup::{CpConfig, run_accessory};
use livi_helper_stack::driver::spawn_link_stream;
use livi_helper_stack::ident::Identity;
use livi_helper_stack::livi_sock::{Broadcaster, SharedTag, pump_artwork, pump_events_for};
use livi_helper_stack::mfi_async::SharedCoprocessor;
use livi_helper_stack::state::HelperState;

use crate::link::LinkPresence;
use crate::short;

fn announce_gone(bcast: &Broadcaster, serial: &str) {
    bcast.push_json(
        HelperEvent::from(HelperKind::DeviceGone {
            src: DeviceSource::Carkit,
            usb_udid: serial.to_string(),
        })
        .to_line(),
    );
}

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const MFI_RETRY: Duration = Duration::from_secs(1);
#[cfg(any(target_os = "linux", target_os = "android"))]
const GIVE_UP_ATTEMPTS: u32 = 3;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub async fn watch(wired: Wired) {
    let registry = Arc::new(MuxRegistry::default());
    let mut failed: HashMap<String, u32> = HashMap::new();
    let mut cancels: HashMap<String, Arc<Notify>> = HashMap::new();
    let mut seen: Vec<String> = Vec::new();
    let rescan = Arc::new(Notify::new());

    loop {
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = wired.link.changed().notified() => {}
            _ = rescan.notified() => {}
        }

        let present: Vec<String> = tokio::task::spawn_blocking(find_iphones)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.serial)
            .collect();
        failed.retain(|serial, _| present.contains(serial));
        for serial in std::mem::replace(&mut seen, present.clone()) {
            if !present.contains(&serial) {
                println!("[wired] {} unplugged", short(&serial));
                if let Some(c) = cancels.remove(&serial) {
                    c.notify_one();
                }
                announce_gone(&wired.bcast, &serial);
                registry.remove(&serial);
            }
        }
        if !wired.link.is_present() {
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
            let dev = match registry.ensure(&serial).await {
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

            let ctx = wired.clone();
            let cancel = Arc::new(Notify::new());
            cancels.insert(serial.clone(), cancel.clone());
            let (registry, rescan) = (registry.clone(), rescan.clone());
            tokio::spawn(async move {
                let ncm = {
                    let (serial, wired) = (dev.serial.clone(), ctx.clone());
                    tokio::task::spawn_blocking(move || start_ncm_bridge(&serial, &wired))
                        .await
                        .unwrap_or(LocalNcm::System(None))
                };
                let serial = dev.serial.clone();
                match open_carkit(&dev, &ctx.lockdown).await {
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
                rescan.notify_one();
            });
        }
    }
}

#[cfg(target_os = "macos")]
pub async fn watch_usbmuxd(wired: Wired) {
    use std::collections::HashSet;

    let mut active: HashMap<String, Arc<Notify>> = HashMap::new();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = wired.link.changed().notified() => {}
        }

        let devices = match iap2_wired::usbmuxd::devices().await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let present: HashSet<String> = devices.iter().map(|d| d.udid.clone()).collect();
        active.retain(|udid, cancel| {
            if wired.state.take_redo(udid) {
                return false;
            }
            let keep = present.contains(udid);
            if !keep {
                println!("[wired] {} unplugged", short(udid));
                cancel.notify_one();
                announce_gone(&wired.bcast, udid);
            }
            keep
        });
        if !wired.link.is_present() {
            continue;
        }

        for device in devices {
            if active.contains_key(&device.udid) {
                continue;
            }
            let cancel = Arc::new(Notify::new());
            active.insert(device.udid.clone(), cancel.clone());
            let (found, late) = tokio::sync::watch::channel(None);
            let mut ctx = wired.clone();
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

// iPadOS names the network it brings up with a USB-connected iPhone anri*.
#[cfg(target_os = "ios")]
const USB_LINK: &str = "anri";
#[cfg(target_os = "ios")]
const REMOTED: &str = "_remoted._tcp";
#[cfg(target_os = "ios")]
const REMOTED_CONNECT: Duration = Duration::from_secs(3);

#[cfg(target_os = "ios")]
struct RemotePhone {
    cancel: Arc<Notify>,
    udid: Arc<std::sync::OnceLock<String>>,
}

#[cfg(target_os = "ios")]
fn usb_phones() -> Vec<(String, u32)> {
    use livi_helper_stack::dnssd;
    dnssd::browse_on(REMOTED, SCAN_INTERVAL)
        .into_iter()
        .filter(|(_, iface)| dnssd::ifname(*iface).is_some_and(|n| n.starts_with(USB_LINK)))
        .collect()
}

#[cfg(target_os = "ios")]
pub async fn watch_remote(wired: Wired) {
    let mut active: HashMap<String, RemotePhone> = HashMap::new();
    let (ended, mut ends) = tokio::sync::mpsc::unbounded_channel::<String>();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = wired.link.changed().notified() => {}
        }
        while let Ok(name) = ends.try_recv() {
            active.remove(&name);
        }

        let present = tokio::task::spawn_blocking(usb_phones)
            .await
            .unwrap_or_default();
        active.retain(|name, phone| {
            let keep = present.iter().any(|(n, _)| n == name);
            if !keep {
                println!("[wired] {name} unplugged");
                phone.cancel.notify_one();
                if let Some(udid) = phone.udid.get() {
                    announce_gone(&wired.bcast, udid);
                }
            }
            keep
        });
        if !wired.link.is_present() {
            continue;
        }

        for (name, iface) in present {
            if active.contains_key(&name) {
                continue;
            }
            let phone = RemotePhone {
                cancel: Arc::new(Notify::new()),
                udid: Default::default(),
            };
            let (cancel, udid) = (phone.cancel.clone(), phone.udid.clone());
            active.insert(name.clone(), phone);
            let (ctx, ended) = (wired.clone(), ended.clone());
            tokio::spawn(async move {
                match open_remote(&name, iface, &ctx.lockdown).await {
                    Ok((carkit, carplay_iface)) => {
                        println!(
                            "[wired] {}: carkit up over the USB link, carplay on {}, starting iAP2",
                            short(&carkit.udid),
                            carplay_iface.as_deref().unwrap_or("-")
                        );
                        let _ = udid.set(carkit.udid.clone());
                        let ncm = LocalNcm::System(carplay_iface);
                        run_wired_session(carkit.udid, carkit.stream, ncm, ctx, cancel).await;
                    }
                    Err(e) => eprintln!("[wired] {name}: carkit failed: {e}"),
                }
                let _ = ended.send(name);
            });
        }
    }
}

// iPadOS brings up two networks with the iPhone: anri* only carries CoreDevice, carplay
// runs on the other one. That is where the phone's own host name resolves besides anri*,
// leaving out the Wi-Fi a shared network adds (iPads have no built-in Ethernet, en0 is Wi-Fi).
#[cfg(target_os = "ios")]
const WIFI: &str = "en0";

#[cfg(target_os = "ios")]
async fn open_remote(
    name: &str,
    iface: u32,
    store: &std::path::Path,
) -> Result<(iap2_wired::remote::RemoteCarkit, Option<String>), String> {
    use livi_helper_stack::dnssd;
    use std::net::{SocketAddr, ToSocketAddrs};
    let instance = name.to_string();
    let addrs: Vec<SocketAddr> = tokio::task::spawn_blocking(move || {
        let found = dnssd::resolve(&instance, REMOTED, iface, SCAN_INTERVAL)?;
        let addrs = (found.host.as_str(), found.port).to_socket_addrs().ok()?;
        Some(addrs.filter(SocketAddr::is_ipv6).collect())
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let scope = |a: &SocketAddr| match a {
        SocketAddr::V6(v) => v.scope_id(),
        SocketAddr::V4(_) => 0,
    };
    let carplay = addrs
        .iter()
        .map(scope)
        .filter(|&s| s != 0 && s != iface)
        .filter_map(dnssd::ifname)
        .find(|name| name != WIFI);
    for addr in addrs.into_iter().filter(|a| scope(a) == iface) {
        if let Ok(Ok(remoted)) =
            tokio::time::timeout(REMOTED_CONNECT, tokio::net::TcpStream::connect(addr)).await
        {
            return iap2_wired::remote::open_carkit(remoted, store)
                .await
                .map(|carkit| (carkit, carplay))
                .map_err(|e| e.to_string());
        }
    }
    Err("remoted unreachable".into())
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
            Err(e) => last = e.to_string(),
        }
        tokio::time::sleep(USB_IFACE_POLL).await;
    }
    eprintln!("[wired] {}: no USB network interface: {last}", short(&udid));
}

#[cfg(target_os = "android")]
const AIRPLAY_NAME: &str = "LIVI";

#[cfg(target_os = "android")]
#[derive(Clone)]
pub struct Airplay {
    pub txt: Vec<String>,
    pub mirror: Option<livi_core_proto::helper::Mirror>,
}

#[derive(Clone, Copy, Default)]
pub struct Dongle {
    pub ap_mac: Option<fn() -> Option<String>>,
    pub bt_mac: Option<fn() -> Option<[u8; 6]>>,
}

#[derive(Clone)]
pub struct Wired {
    pub auth: SharedCoprocessor,
    pub identity: Identity,
    pub cp: CpConfig,
    pub dongle: Dongle,
    pub bcast: Broadcaster,
    pub state: Arc<HelperState>,
    pub link: Arc<LinkPresence>,
    pub lockdown: PathBuf,
    #[cfg(target_os = "android")]
    pub tunnel: Arc<iap2_usbmux::Tunnel>,
    #[cfg(target_os = "android")]
    pub airplay: Airplay,
}

async fn run_wired_session<S>(
    serial: String,
    stream: S,
    ncm: LocalNcm,
    ctx: Wired,
    cancel: Arc<Notify>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut cp = ctx.cp;
    if let Some(name) = ncm.ifname() {
        cp.av_iface = Some(name.to_string());
    }
    cp.av_address = ncm.address().map(|a| a.to_string());
    #[cfg(target_os = "android")]
    let _mdns = ncm
        .ifname()
        .zip(ncm.address())
        .map(|(ifname, address)| announce(ifname, address, &cp, &ctx.airplay, true));
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
            println!("[wired] {}: no CarPlay session came up", short(&serial));
            usb_reset = cfg!(usb_host);
        }
    }
    forward.abort();
    ctx.state.wired_ended(&serial);
    ctx.state.carkit_ended(&ident);
    drop(ncm);
    if usb_reset {
        println!("[wired] {}: resetting the phone's USB", short(&serial));
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
    use livi_helper_stack::AsyncAuth;
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

enum LocalNcm {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Local(iap2_usbmux::NcmBridge),
    System(Option<String>),
}

impl LocalNcm {
    fn ifname(&self) -> Option<&str> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            LocalNcm::Local(b) => Some(b.ifname.as_str()),
            LocalNcm::System(name) => name.as_deref(),
        }
    }

    fn address(&self) -> Option<std::net::Ipv6Addr> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            LocalNcm::Local(b) => b.address,
            LocalNcm::System(_) => None,
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) fn announce(
    ifname: &str,
    address: std::net::Ipv6Addr,
    cp: &CpConfig,
    airplay: &Airplay,
    carplay: bool,
) -> Vec<livi_mdns::responder::Responder> {
    use livi_helper_stack::bonjour::AIRPLAY_SERVICE;
    let carplay = u16::try_from(cp.airplay_port)
        .ok()
        .filter(|&p| carplay && p != 0)
        .map(|port| (AIRPLAY_NAME, port, airplay.txt.as_slice()));
    let mirror = airplay
        .mirror
        .as_ref()
        .map(|m| (m.name.as_str(), m.port, m.txt.as_slice()));
    carplay
        .into_iter()
        .chain(mirror)
        .filter_map(|(name, port, txt)| {
            let service = livi_mdns::service::Service::new(
                name,
                AIRPLAY_SERVICE,
                AIRPLAY_NAME,
                port,
                txt,
                address,
            );
            match livi_mdns::responder::Responder::start(ifname, service) {
                Ok(responder) => {
                    println!("[mdns] {AIRPLAY_SERVICE} {name:?} port={port} on {ifname}");
                    Some(responder)
                }
                Err(e) => {
                    eprintln!("[mdns] {AIRPLAY_SERVICE} {name:?} not announced on {ifname}: {e}");
                    None
                }
            }
        })
        .collect()
}

#[cfg(target_os = "android")]
fn start_ncm_bridge(serial: &str, wired: &Wired) -> LocalNcm {
    bridged(serial, iap2_usbmux::NcmBridge::start(serial, &wired.tunnel))
}

#[cfg(target_os = "linux")]
fn start_ncm_bridge(serial: &str, _wired: &Wired) -> LocalNcm {
    bridged(serial, iap2_usbmux::NcmBridge::start(serial))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn bridged(serial: &str, bridge: Result<iap2_usbmux::NcmBridge, iap2_usbmux::Error>) -> LocalNcm {
    match bridge {
        Ok(b) => LocalNcm::Local(b),
        Err(e) => {
            eprintln!("[wired] {}: ncm bridge unavailable: {e}", short(serial));
            LocalNcm::System(None)
        }
    }
}
