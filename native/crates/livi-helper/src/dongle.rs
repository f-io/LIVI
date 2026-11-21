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
use livi_link_host::link::CHOICE;

pub const AP_WAIT: Duration = Duration::from_secs(15);
const AP_POLL: Duration = Duration::from_millis(250);

pub struct Radios {
    pub name: String,
    pub passphrase: String,
    pub channel: Channel,
    pub port: u16,
    pub carplay: bool,
    pub android_auto: bool,
    pub module: bool,
    pub wifi: String,
    pub sockets: HelperSockets,
    pub keys: PathBuf,
}

pub fn phones_iface(wifi: &str) -> String {
    if wifi != CHOICE {
        return wifi.to_string();
    }
    livi_link_host::link::name()
        .and_then(|name| livi_helper_stack::net::iface_facing(&name))
        .unwrap_or_default()
}

pub fn ap_on(wifi: &str) -> bool {
    if wifi == CHOICE {
        livi_link_host::ap::status_field("state").as_deref() == Some("on")
    } else {
        livi_wifi::on_air(wifi).is_some()
    }
}

pub fn ap_ready(wifi: &str, within: Duration) -> bool {
    if wifi == CHOICE {
        return livi_link_host::ap::ready(within);
    }
    let deadline = std::time::Instant::now() + within;
    while !ap_on(wifi) {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(AP_POLL);
    }
    true
}

pub fn wireless_config(base: &CpConfig, wifi: &str) -> CpConfig {
    if wifi != CHOICE {
        let live = livi_wifi::on_air(wifi);
        return CpConfig {
            ap_mac: livi_helper_stack::net::wlan_mac(wifi),
            ssid: live
                .as_ref()
                .map(|ap| ap.ssid.clone())
                .unwrap_or_else(|| base.ssid.clone()),
            wifi_iface: wifi.to_string(),
            channel: live.map_or(base.channel, |ap| ap.channel),
            transport: Transport::Wireless,
            ..base.clone()
        };
    }
    CpConfig {
        ap_mac: livi_link_host::ap::mac(),
        ssid: livi_link_host::ap::ssid().unwrap_or_else(|| base.ssid.clone()),
        wifi_iface: phones_iface(wifi),
        channel: livi_link_host::ap::status_field("channel")
            .and_then(|c| c.parse().ok())
            .map_or(base.channel, Channel::of_number),
        ap_on_air: Some(livi_link_host::ap::on_air as AskOnAir),
        transport: Transport::Wireless,
        ..base.clone()
    }
}

pub fn wireless_identity(base: &Identity, module: bool) -> Identity {
    let bt_mac = if module {
        livi_core_proto::helper::module_bt_mac()
    } else {
        livi_link_host::ap::bt_mac()
    };
    Identity {
        bt_mac: bt_mac.unwrap_or(base.bt_mac),
        ..base.clone()
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn wireless_sessions(
    auth: SharedCoprocessor,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    mut arrivals: UnboundedReceiver<Arrival>,
    state: Arc<HelperState>,
    dongle: Dongle,
    wifi: String,
) {
    while let Some(session) = arrivals.recv().await {
        let ready = wifi.clone();
        if !tokio::task::spawn_blocking(move || ap_ready(&ready, AP_WAIT))
            .await
            .unwrap_or(false)
        {
            eprintln!("[helper] the access point is not up, not starting a session");
            continue;
        }
        let (base, wifi) = (cp.clone(), wifi.clone());
        let resolved = tokio::task::spawn_blocking(move || wireless_config(&base, &wifi))
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
        #[cfg(target_os = "linux")]
        wifi_iface: phones_iface(&radios.wifi),
        ap_ip: String::new(),
        port: radios.port,
    };
    tokio::spawn(crate::aa::watch_dongle(
        arrivals,
        cfg,
        radios.wifi.clone(),
        events,
        wired,
        state,
    ));
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
    #[cfg(feature = "uart-module")]
    if radios.module {
        tokio::spawn(crate::uart_module::run(
            radios.sockets.module,
            radios.name,
            radios.wifi.clone(),
            crate::uart_module::Profiles {
                carplay: profiles.carplay,
                android_auto: profiles.android_auto,
                hfp: profiles.hfp,
            },
        ));
        tokio::spawn(livi_helper_stack::reconnect::run(
            crate::uart_module::Pager::new(radios.wifi.clone()),
            state,
        ));
        return radios.carplay.then_some(carplay_arrivals);
    }
    {
        let radio = Radio {
            name: radios.name,
            keys: radios.keys,
            mic: radios.sockets.sco_mic,
            offered,
        };
        tokio::spawn(crate::bt_dongle::run(dongle, link, profiles, state, radio));
        println!("[helper] hands-free over the dongle's bluetooth is on");
    }
    radios.carplay.then_some(carplay_arrivals)
}
