// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iap2_link::LinkConfig;
use iap2_mfi::{NcmCoprocessor, NoCoprocessor};
use livi_bt_host::sdp::Service;
use livi_core_proto::helper::HelperSockets;
use livi_runtime::bringup::{AskOnAir, CpConfig, run_accessory};
use livi_runtime::driver::spawn_link_stream;
use livi_runtime::hfp::Hfp;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{Broadcaster, SharedTag, pump_artwork, pump_events_for};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::sco::ScoSink;
use livi_runtime::state::HelperState;
use livi_wifi::Channel;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::aa::{AaConfig, WiredPhones};
use crate::bt_dongle::{Arrival, Dongle, Profiles, Radio};
use crate::link::LinkPresence;

const AP_WAIT: Duration = Duration::from_secs(15);
const MFI_RETRY: Duration = Duration::from_secs(2);

pub struct Radios {
    pub name: String,
    pub passphrase: String,
    pub channel: Channel,
    pub port: u16,
    pub carplay: bool,
    pub android_auto: bool,
    pub sockets: HelperSockets,
    pub keys: PathBuf,
}

pub fn wireless_config(base: &CpConfig) -> CpConfig {
    CpConfig {
        ap_mac: livi_link_host::ap::mac(),
        ssid: livi_link_host::ap::ssid().unwrap_or_else(|| base.ssid.clone()),
        wifi_iface: livi_runtime::net::iface_facing(livi_link_host::link::LINK_NAME)
            .unwrap_or_default(),
        channel: livi_link_host::ap::status_field("channel")
            .and_then(|c| c.parse().ok())
            .map_or(base.channel, Channel::of_number),
        ap_on_air: Some(livi_link_host::ap::on_air as AskOnAir),
        transport: Transport::Wireless,
        ..base.clone()
    }
}

pub fn wireless_identity(base: &Identity) -> Identity {
    Identity {
        bt_mac: livi_link_host::ap::bt_mac().unwrap_or(base.bt_mac),
        ..base.clone()
    }
}

pub async fn wireless_sessions(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    mut arrivals: UnboundedReceiver<Arrival>,
    state: Arc<HelperState>,
    dongle: Dongle,
) {
    while let Some(session) = arrivals.recv().await {
        if state.carkit_claims(&session.peer) {
            println!(
                "[helper] {} is on the cable, its Bluetooth link goes",
                session.peer
            );
            if let Err(e) = dongle.disconnect(&session.peer) {
                eprintln!(
                    "[helper] {} stays on the dongle's bluetooth: {e}",
                    session.peer
                );
            }
            continue;
        }
        if !tokio::task::spawn_blocking(|| livi_link_host::ap::ready(AP_WAIT))
            .await
            .unwrap_or(false)
        {
            eprintln!("[helper] the dongle's access point is not up, not starting a session");
            continue;
        }
        let cp = CpConfig {
            on_cable: Some(dongle.on_cable(state.clone())),
            ..wireless_config(&cp)
        };
        let identity = Identity {
            bt_mac: session.local,
            ..identity.clone()
        };
        println!(
            "[helper] phone connected mac={} over the dongle, ap {} channel {}",
            session.peer,
            cp.ap_mac.clone().unwrap_or_default(),
            cp.channel
        );
        let cfg = LinkConfig {
            max_outgoing: 4,
            control_version: 2,
            ..LinkConfig::default()
        };
        let (channel, art_rx) = spawn_link_stream(session.stream, cfg, false);
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(run_accessory(
            channel,
            auth.clone(),
            identity,
            cp,
            tx,
            state.vehicle_feed(),
        ));
        let ident: SharedTag = Default::default();
        tokio::spawn(pump_events_for(
            rx,
            bcast.clone(),
            "bt",
            None,
            ident.clone(),
        ));
        tokio::spawn(pump_artwork(art_rx, bcast.clone(), ident));
    }
}

pub async fn identify_on_link(link: Arc<LinkPresence>, mut auth: SharedCoprocessor) {
    use livi_runtime::AsyncAuth;
    loop {
        link.wait_until(true).await;
        let mut reported = false;
        let major = loop {
            if !link.is_present() {
                break None;
            }
            match auth.protocol_major().await {
                Ok(major) => break Some(major),
                Err(e) => {
                    if !reported {
                        eprintln!("[helper] MFi coprocessor not answering yet: {e}");
                        reported = true;
                    }
                    tokio::time::sleep(MFI_RETRY).await;
                }
            }
        };
        let Some(major) = major else { continue };
        let kind = if major == 2 {
            "2.0 (RSA, SHA-1)"
        } else {
            "3.0 (ECDSA, SHA-256)"
        };
        println!("[helper] MFi coprocessor: auth protocol major {major} — {kind}");
        link.wait_until(false).await;
    }
}

pub fn follow_link(
    link: Arc<LinkPresence>,
    auth: SharedCoprocessor,
    own_files: bool,
    bcast: Broadcaster,
    dongle: Dongle,
) {
    let (up_auth, down_auth) = (auth.clone(), auth);
    let (arrived, left) = (bcast.clone(), bcast);
    tokio::spawn(link.resolve(
        move || {
            if !own_files {
                up_auth.replace(Box::new(NcmCoprocessor::new(&livi_link_host::link::addr(
                    livi_net::port::MFI,
                ))));
            }
            arrived.push_json("{\"type\":\"link\",\"up\":true}".into());
        },
        move || {
            if !own_files {
                down_auth.replace(Box::new(NoCoprocessor));
            }
            dongle.end();
            println!("[helper] the dongle went");
            left.push_json("{\"type\":\"link\",\"up\":false}".into());
        },
    ));
}

fn start_android_auto(
    radios: &Radios,
    arrivals: UnboundedReceiver<Arrival>,
    events: Broadcaster,
    wired: WiredPhones,
    state: Arc<HelperState>,
) {
    let sessions = events.clone();
    tokio::spawn(livi_aa::server::run(
        radios.port,
        radios.sockets.sessions.clone(),
        move |socket, peer| {
            sessions.push_json(format!(
            "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"wifi\"}}"
        ));
        },
    ));
    let cfg = AaConfig {
        ssid: radios.name.clone(),
        passphrase: radios.passphrase.clone(),
        channel: radios.channel,
        ap_ip: String::new(),
        port: radios.port,
    };
    tokio::spawn(crate::aa::watch_dongle(arrivals, cfg, events, wired, state));
    println!("[helper] wireless Android Auto over the dongle's bluetooth is on");
}

pub fn start_bluetooth(
    radios: Radios,
    link: Arc<LinkPresence>,
    state: Arc<HelperState>,
    dongle: Dongle,
    events: Broadcaster,
    wired: WiredPhones,
    sco_sink: ScoSink,
) -> Option<UnboundedReceiver<Arrival>> {
    let (carplay, carplay_arrivals) = unbounded_channel();
    let (android_auto, aa_arrivals) = unbounded_channel();
    let hfp = Hfp::default();
    hfp.set_events(events.clone());
    let mut offered = vec![Service::HandsFree];
    if radios.carplay {
        offered.push(Service::CarPlay);
    }
    if radios.android_auto {
        offered.push(Service::AndroidAuto);
        start_android_auto(&radios, aa_arrivals, events.clone(), wired, state.clone());
    }
    let profiles = Profiles {
        carplay: radios.carplay.then_some(carplay),
        android_auto: radios.android_auto.then_some(android_auto),
        hfp,
        events,
        sco_sink,
    };
    let radio = Radio {
        name: radios.name,
        keys: radios.keys,
        mic: radios.sockets.sco_mic,
        offered,
    };
    tokio::spawn(crate::bt_dongle::run(dongle, link, profiles, state, radio));
    println!("[helper] hands-free over the dongle's bluetooth is on");
    radios.carplay.then_some(carplay_arrivals)
}
