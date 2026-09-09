// Wired CarPlay: one iAP2 session per iPhone on the USB bus.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use iap2_link::LinkConfig;
use iap2_usbmux::{MuxRegistry, try_find_iphones};
use iap2_wired::open_carkit;
use livi_runtime::bringup::{CpConfig, run_accessory};
use livi_runtime::driver::spawn_link_stream;
use livi_runtime::ident::Identity;
use livi_runtime::livi_sock::{Broadcaster, SharedTag, pump_artwork, pump_events_for};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::state::HelperState;

use crate::link::LinkPresence;

/// Tell the app a wired phone is gone.
fn announce_gone(bcast: &Broadcaster, serial: &str) {
    bcast.push_json(format!(
        "{{\"type\":\"device-gone\",\"src\":\"carkit\",\"usbUdid\":\"{serial}\"}}"
    ));
}

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
// A device that keeps failing the config probe is no iPhone (e.g. a dongle emulating one).
const GIVE_UP_ATTEMPTS: u32 = 3;

pub async fn watch(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    link: Arc<LinkPresence>,
) {
    // A phone stays in the registry while its session runs. The session removes it when it ends.
    let registry = Arc::new(MuxRegistry::default());
    let mut failed: HashMap<String, u32> = HashMap::new();
    // serial -> cancel handle for its running session, fired when the phone leaves the bus.
    let mut cancels: HashMap<String, Arc<Notify>> = HashMap::new();

    loop {
        if !link.is_present() {
            // No MFi without the link: every phone is retired until it is back.
            for serial in registry.serials() {
                println!("[wired] {} gone with the link", short(&serial));
                if let Some(c) = cancels.remove(&serial) {
                    c.notify_one();
                }
                announce_gone(&bcast, &serial);
                registry.remove(&serial);
            }
            link.wait_until(true).await;
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = link.changed().notified() => {}
        }
        if !link.is_present() {
            continue;
        }

        // An unreachable proxy on a present link is a hiccup: nothing is retired on it.
        let Ok(found) = try_find_iphones() else {
            continue;
        };
        let present: Vec<String> = found.into_iter().map(|d| d.serial).collect();
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
                bcast: bcast.clone(),
                state: state.clone(),
            };
            let cancel = Arc::new(Notify::new());
            cancels.insert(serial.clone(), cancel.clone());
            let registry = registry.clone();
            tokio::spawn(async move {
                // The AV stream rides the phone's USB network function, whose link-local
                // address is what CarPlayStartSession hands back to the phone.
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

/// The phone directly on a Mac port, reached through the system usbmuxd instead of the dongle's
/// proxy. Same iAP2 session, MFi still from the dongle, so it also waits for the link.
#[cfg(target_os = "macos")]
pub async fn watch_usbmuxd(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    link: Arc<LinkPresence>,
) {
    use std::collections::HashSet;

    // UDID -> cancel handle for its running session. Fired when the phone leaves the bus, so the
    // session ends instead of hanging on a carkit stream usbmuxd keeps open. One attempt per plug-in.
    let mut active: HashMap<String, Arc<Notify>> = HashMap::new();

    loop {
        if !link.is_present() {
            for (udid, cancel) in active.drain() {
                cancel.notify_one();
                announce_gone(&bcast, &udid);
            }
            link.wait_until(true).await;
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
            _ = link.changed().notified() => {}
        }
        if !link.is_present() {
            continue;
        }

        let devices = match iap2_wired::usbmuxd::devices().await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let present: HashSet<String> = devices.iter().map(|d| d.udid.clone()).collect();
        active.retain(|udid, cancel| {
            let keep = present.contains(udid);
            if !keep {
                println!("[wired] {} unplugged", short(udid));
                cancel.notify_one();
                announce_gone(&bcast, udid);
            }
            keep
        });

        for device in devices {
            if active.contains_key(&device.udid) {
                continue;
            }
            let cancel = Arc::new(Notify::new());
            active.insert(device.udid.clone(), cancel.clone());
            let ctx = WiredCtx {
                auth: auth.clone(),
                identity: identity.clone(),
                cp: cp.clone(),
                bcast: bcast.clone(),
                state: state.clone(),
            };
            tokio::spawn(async move {
                // The phone's own USB network interface (enX), resolved from the UDID.
                let ncm =
                    LocalNcm::Bridged(iap2_wired::mac_network::discover(&device.udid).await.ok());
                match iap2_wired::usbmuxd::open(&device).await {
                    Ok(stream) => {
                        println!(
                            "[wired] {}: usbmuxd carkit up, starting iAP2",
                            short(&device.udid)
                        );
                        run_wired_session(device.udid.clone(), stream, ncm, ctx, cancel).await;
                    }
                    Err(e) => eprintln!(
                        "[wired] {}: usbmuxd carkit failed: {e}",
                        short(&device.udid)
                    ),
                }
            });
        }
    }
}

/// What a session needs beyond the phone itself, cloned per phone.
#[derive(Clone)]
struct WiredCtx {
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    state: Arc<HelperState>,
}

/// The transport-agnostic half: from an open iAP2 stream through identification, MFi auth and the
/// CarPlay session. Both watchers feed it the same way.
async fn run_wired_session<S>(
    serial: String,
    stream: S,
    ncm: LocalNcm,
    ctx: WiredCtx,
    cancel: Arc<Notify>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let cp = match ncm.ifname() {
        Some(name) => CpConfig {
            av_iface: Some(name.to_string()),
            ..ctx.cp
        },
        None => ctx.cp,
    };
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
    tokio::spawn(pump_events_for(
        rx,
        ctx.bcast.clone(),
        "wired",
        Some(serial.clone()),
        ident.clone(),
    ));
    tokio::spawn(pump_artwork(art_rx, ctx.bcast, ident.clone()));
    // End on either the phone closing iAP2 or the watcher cancelling on unplug, so the session
    // and its state never outlive the physical connection.
    tokio::select! {
        _ = run_accessory(ch, ctx.auth, ctx.identity, cp, tx) => {}
        _ = cancel.notified() => println!("[wired] {}: session cancelled on unplug", short(&serial)),
    }
    ctx.state.carkit_ended(&ident);
    drop(ncm);
}

fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}

/// Where the phone's USB network function shows up for this session.
enum LocalNcm {
    /// Phone on this machine's bus: brought up here.
    #[cfg(target_os = "linux")]
    Local(iap2_usbmux::NcmBridge),
    /// Phone on a dongle: bridged onto the interface facing it.
    Bridged(Option<String>),
}

impl LocalNcm {
    fn ifname(&self) -> Option<&str> {
        match self {
            #[cfg(target_os = "linux")]
            LocalNcm::Local(b) => Some(b.ifname.as_str()),
            LocalNcm::Bridged(name) => name.as_deref(),
        }
    }
}

fn start_ncm_bridge(serial: &str) -> LocalNcm {
    let _ = serial;
    if let Some(addr) = iap2_usbmux::remote_addr() {
        let iface = livi_runtime::net::iface_facing(&addr);
        if iface.is_none() {
            eprintln!("[wired] no interface facing the LIVI Link at {addr} yet");
        }
        return LocalNcm::Bridged(iface);
    }
    #[cfg(target_os = "linux")]
    {
        match iap2_usbmux::NcmBridge::start(serial) {
            Ok(b) => LocalNcm::Local(b),
            Err(e) => {
                eprintln!("[wired] {}: ncm bridge unavailable: {e}", short(serial));
                LocalNcm::Bridged(None)
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    LocalNcm::Bridged(None)
}
