// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use livi_bt_host::sdp::Service;
use livi_core_proto::helper::HelperSockets;
use livi_helper_stack::bringup::{AskOnAir, CpConfig};
use livi_helper_stack::driver::spawn_link_stream;
use livi_helper_stack::hfp::Hfp;
use livi_helper_stack::ident::{Identity, Transport};
use livi_helper_stack::livi_sock::{Broadcaster, run_bluetooth};
use livi_helper_stack::mfi_async::SharedCoprocessor;
use livi_helper_stack::sco::ScoSink;
use livi_helper_stack::state::HelperState;
use livi_wifi::Channel;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::aa::{AaConfig, WiredPhones};
use crate::bt_dongle::{Arrival, Dongle, Profiles, Radio};
use crate::link::LinkPresence;

const AP_WAIT: Duration = Duration::from_secs(15);

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
        wifi_iface: livi_link_host::link::name()
            .and_then(|name| livi_helper_stack::net::iface_facing(&name))
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
        if !tokio::task::spawn_blocking(|| livi_link_host::ap::ready(AP_WAIT))
            .await
            .unwrap_or(false)
        {
            eprintln!("[helper] the dongle's access point is not up, not starting a session");
            continue;
        }
        let base = cp.clone();
        let resolved = tokio::task::spawn_blocking(move || wireless_config(&base))
            .await
            .unwrap_or_else(|_| cp.clone());
        let cp = CpConfig {
            on_cable: Some(dongle.on_cable(state.clone())),
            ..resolved
        };
        let identity = Identity {
            bt_mac: session.local,
            ..identity.clone()
        };
        println!(
            "[helper] {} is on the dongle, ap {} channel {}",
            session.peer,
            cp.ap_mac.clone().unwrap_or_default(),
            cp.channel
        );
        let peer = session.peer.to_string();
        run_bluetooth(
            |cfg| spawn_link_stream(session.stream, cfg, false),
            auth.clone(),
            identity,
            cp,
            bcast.clone(),
            state.clone(),
            peer,
        );
    }
}

fn start_android_auto(
    radios: &Radios,
    arrivals: UnboundedReceiver<Arrival>,
    events: Broadcaster,
    wired: WiredPhones,
    state: Arc<HelperState>,
) {
    crate::aa::serve_wifi(radios.port, radios.sockets.sessions.clone(), events.clone());
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
