// Wired CarPlay: one iAP2 session per iPhone on the USB bus.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iap2_link::LinkConfig;
use livi_runtime::bringup::{run_accessory, CpConfig};
use livi_runtime::driver::spawn_link_stream;
use livi_runtime::ident::Identity;
use livi_runtime::livi_sock::{pump_artwork, pump_events_for, Broadcaster, SharedTag};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::state::HelperState;
use iap2_usbmux::{try_find_iphones, MuxRegistry};
use iap2_wired::open_carkit;

use crate::link::LinkPresence;

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
    // A phone stays in the registry while its session runs; the session removes it when it ends.
    let registry = Arc::new(MuxRegistry::default());
    let mut failed: HashMap<String, u32> = HashMap::new();

    loop {
        if !link.is_present() {
            // No MFi without the link: every phone is retired until it is back.
            for serial in registry.serials() {
                println!("[wired] {} gone with the link", short(&serial));
                bcast.push_json(format!(
                    "{{\"type\":\"device-gone\",\"src\":\"carkit\",\"usbUdid\":\"{serial}\"}}"
                ));
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
        let Ok(found) = try_find_iphones() else { continue };
        let present: Vec<String> = found.into_iter().map(|d| d.serial).collect();
        failed.retain(|serial, _| present.contains(serial));
        for serial in registry.serials() {
            if !present.contains(&serial) {
                println!("[wired] {} unplugged", short(&serial));
                bcast.push_json(format!(
                    "{{\"type\":\"device-gone\",\"src\":\"carkit\",\"usbUdid\":\"{serial}\"}}"
                ));
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

            let (auth, identity, cp, bcast, state) =
                (auth.clone(), identity.clone(), cp.clone(), bcast.clone(), state.clone());
            let registry = registry.clone();
            tokio::spawn(async move {
                // The AV stream rides the phone's USB network function, whose link-local
                // address is what CarPlayStartSession hands back to the phone.
                let ncm = start_ncm_bridge(&dev.serial);
                let cp = match ncm.ifname() {
                    Some(name) => CpConfig { av_iface: Some(name.to_string()), ..cp },
                    None => cp,
                };

                match open_carkit(&dev).await {
                    Ok(channel) => match channel.into_stream() {
                        Some(stream) => {
                            println!("[wired] {}: carkit channel up, starting iAP2", short(&dev.serial));
                            let link = LinkConfig {
                                max_outgoing: 4,
                                control_version: 2,
                                zero_ack: true,
                                ..LinkConfig::default()
                            };
                            let (ch, art_rx) = spawn_link_stream(stream, link, true);
                            let (tx, rx) = tokio::sync::mpsc::channel(64);
                            let ident: SharedTag = Default::default();
                            state.carkit_started(ident.clone());
                            tokio::spawn(pump_events_for(
                                rx,
                                bcast.clone(),
                                "wired",
                                Some(dev.serial.clone()),
                                ident.clone(),
                            ));
                            tokio::spawn(pump_artwork(art_rx, bcast, ident.clone()));
                            run_accessory(ch, auth, identity, cp, tx).await;
                            state.carkit_ended(&ident);
                        }
                        None => eprintln!("[wired] {}: carkit stream unavailable", short(&dev.serial)),
                    },
                    Err(e) => eprintln!("[wired] {}: carkit failed: {e}", short(&dev.serial)),
                }
                drop(ncm);
                registry.remove(&dev.serial);
            });
        }
    }
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
