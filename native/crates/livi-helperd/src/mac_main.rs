// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::helper::{HelperSockets, HelperStart};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use iap2_mfi::NoCoprocessor;
use livi_runtime::bonjour::Bonjour;
use livi_runtime::bringup::CpConfig;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{self, Broadcaster, LiviSockConfig};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::sco::ScoSink;
use livi_runtime::state::HelperState;
use livi_wifi::Channel;
use tokio::sync::mpsc::UnboundedReceiver;

use livi_helper::bt_dongle::{Arrival, Dongle};
use livi_helper::dongle::{
    Radios, follow_link, identify_on_link, start_bluetooth, wireless_config, wireless_identity,
    wireless_sessions,
};
use livi_helper::link::LinkPresence;
use livi_helper::wired::Wired;
use livi_helper::{DEFAULT_CHANNEL, DEFAULT_NAME, DEFAULT_PASSPHRASE};

static BONJOUR: std::sync::OnceLock<Bonjour> = std::sync::OnceLock::new();

fn env_s(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn accessory_mac(pi: &str) -> [u8; 6] {
    if let Ok(s) = std::env::var("LIVI_CP_BT_MAC") {
        let bytes: Vec<u8> = s
            .split(':')
            .filter_map(|h| u8::from_str_radix(h, 16).ok())
            .collect();
        if bytes.len() == 6 {
            return [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]];
        }
    }
    let mut m = [0x02u8, 0, 0, 0, 0, 0];
    for (i, b) in pi.bytes().enumerate() {
        m[1 + (i % 5)] ^= b;
    }
    m
}

fn cp_config() -> (CpConfig, Identity) {
    let start = HelperStart::from_env();
    let (mut identity, cp) = livi_helper::carplay(&start, Transport::Wired);
    identity.bt_mac = accessory_mac(&start.pairing_id);
    (cp, identity)
}

fn wireless_android_auto() -> bool {
    env_s("LIVI_AA_WIRELESS", "1") != "0"
}

fn radios() -> Radios {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    Radios {
        name: env_s("LIVI_CP_NAME", DEFAULT_NAME),
        passphrase: env_s("LIVI_PASSPHRASE", DEFAULT_PASSPHRASE),
        channel: Channel::of_number(env_s("LIVI_CHANNEL", "").parse().unwrap_or(DEFAULT_CHANNEL)),
        port: env_s("LIVI_PORT", "")
            .parse()
            .unwrap_or(livi_aa::consts::TCP_PORT),
        carplay: env_s("LIVI_CP_WIRELESS", "") == "1",
        android_auto: wireless_android_auto(),
        sockets: HelperSockets::system(),
        keys: home.join(".config/LIVI/bluetooth-keys"),
    }
}

fn over_dongle() -> bool {
    env_s("LIVI_BT_ADAPTER", "") == livi_link_host::link::CHOICE
}

fn start_carplay_seam(
    link: Arc<LinkPresence>,
    state: Arc<HelperState>,
    dongle: Dongle,
    arrivals: Option<UnboundedReceiver<Arrival>>,
) {
    let (cp, identity) = cp_config();
    let files = crate::mfi_files::open();
    let own_files = files.is_some();
    let auth = match files {
        Some(files) => SharedCoprocessor::new(Box::new(files)),
        None => SharedCoprocessor::new(Box::new(NoCoprocessor)),
    };
    let bcast = Broadcaster::default();

    let over_dongle = over_dongle();

    follow_link(
        link.clone(),
        auth.clone(),
        own_files,
        bcast.clone(),
        dongle.clone(),
    );
    let sock_cfg = LiviSockConfig {
        path: HelperSockets::system().carplay,
        identity: if over_dongle {
            wireless_identity(&identity)
        } else {
            identity.clone()
        },
        cp: if over_dongle {
            wireless_config(&cp)
        } else {
            cp.clone()
        },
        disconnect: over_dongle.then(|| {
            let dongle = dongle.clone();
            Arc::new(move |mac: String| dongle.disconnect(&mac)) as _
        }),
        targets: None,
        cp_live: over_dongle.then(|| {
            let base = cp.clone();
            Arc::new(move || wireless_config(&base)) as _
        }),
    };
    let (bc, st, a) = (bcast.clone(), state.clone(), auth.clone());
    tokio::spawn(async move {
        let no_bluez = tokio::sync::watch::channel(None).1;
        if let Err(e) = livi_sock::serve(sock_cfg, a, no_bluez, bc, st).await {
            eprintln!("[helperd] livi_sock ended: {e}");
        }
    });

    let (auth_wireless, identity_wireless, cp_wireless) =
        (auth.clone(), identity.clone(), cp.clone());
    tokio::spawn(identify_on_link(link.clone(), auth.clone()));
    let dongle_ap = env_s("LIVI_WIFI_IFACE", "") == livi_link_host::link::CHOICE;
    tokio::spawn(livi_helper::wired::watch_usbmuxd(Wired {
        auth,
        identity,
        cp: cp.clone(),
        dongle: livi_helper::wired::Dongle {
            ap_mac: dongle_ap.then_some(livi_link_host::ap::mac as fn() -> Option<String>),
            bt_mac: over_dongle.then_some(livi_link_host::ap::bt_mac as fn() -> Option<[u8; 6]>),
        },
        bcast: bcast.clone(),
        state: state.clone(),
        link: if own_files {
            LinkPresence::always()
        } else {
            link.clone()
        },
        lockdown: crate::lockdown_dir(),
    }));
    println!("[helperd] wired CarPlay watcher started (system usbmuxd), waiting for the LIVI Link");
    if dongle_ap {
        tokio::spawn(livi_helper::link::relay_stations(bcast.clone()));
    }
    if let Some(arrivals) = arrivals {
        tokio::spawn(wireless_sessions(
            auth_wireless,
            identity_wireless,
            cp_wireless,
            bcast.clone(),
            arrivals,
            state,
            dongle,
        ));
        println!("[helperd] wireless CarPlay over the dongle's bluetooth is on");
    }

    if cp.airplay_port == 0 {
        eprintln!("[helperd] LIVI opened no CarPlay port, CarPlay is not announced");
        return;
    }
    let pk = env_s("LIVI_CP_PK", "");
    let pi = env_s("LIVI_CP_PI", "");
    let device_id = cp.device_id.clone().unwrap_or_else(|| {
        let [a, b, c, d, e, f] = accessory_mac(&pi);
        format!("{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}")
    });
    match Bonjour::start(
        device_id,
        cp.airplay_port as u16,
        cp.source_version.clone(),
        pk,
        pi,
        bcast,
    ) {
        Ok(b) => {
            let _ = BONJOUR.set(b);
            println!(
                "[helperd] CarPlay receiver seam ready (cp-bt.sock + bonjour :{})",
                cp.airplay_port
            );
        }
        Err(e) => eprintln!("[helperd] bonjour start failed: {e}"),
    }
}

pub fn run() -> ExitCode {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(async {
        let dongle_ap = env_s("LIVI_WIFI_IFACE", "") == livi_link_host::link::CHOICE;
        let shared = livi_runtime::shared_sock::SharedSockDeps {
            adapter: String::new(),
            wifi_iface: String::new(),
            events: Broadcaster::default(),
            set_playback_status: Box::new(|_| {}),
            deauth_dongle: dongle_ap.then_some(livi_link_host::ap::deauth as fn() -> Option<usize>),
            bluetooth: None,
        };
        tokio::spawn(async move {
            let path = HelperSockets::system().shared;
            if let Err(e) = livi_runtime::shared_sock::serve(&path, None, shared).await {
                eprintln!("[shared-sock] ended: {e}");
            }
        });
        let aa_events = Broadcaster::default();
        let usb_control = livi_aa::usb::Control::default();
        let wired_phones = livi_helper::aa::WiredPhones::default();
        let sco_sink = ScoSink::default();
        let deps = livi_runtime::aa_sock::AaSockDeps {
            set_wired_phones: Box::new({
                let wired = wired_phones.clone();
                move |ids| wired.set(ids)
            }),
            restart_usb: Box::new({
                let usb = usb_control.clone();
                move |serial| usb.restart(serial)
            }),
            events: aa_events.clone(),
            set_sco_sink: Box::new({
                let sink = sco_sink.clone();
                move |target| sink.set(target)
            }),
        };
        tokio::spawn(async move {
            if let Err(e) =
                livi_runtime::aa_sock::serve(&HelperSockets::system().android_auto, deps).await
            {
                eprintln!("[aa-sock] ended: {e}");
            }
        });
        tokio::spawn(livi_helper::aa::watch_usb(
            usb_control,
            HelperSockets::system().sessions,
            aa_events.clone(),
        ));
        println!("[helperd] Android Auto USB watcher started");
        let link = LinkPresence::new();
        let link_state = link.clone();
        tokio::spawn(livi_link_host::run(move |on, _serial| {
            link_state.set_on_bus(on)
        }));
        println!("[helperd] dongle watcher started");

        let state = Arc::new(HelperState::default());
        let dongle = Dongle::default();
        let carplay = if over_dongle() {
            start_bluetooth(
                radios(),
                link.clone(),
                state.clone(),
                dongle.clone(),
                aa_events,
                wired_phones,
                sco_sink,
            )
        } else {
            None
        };
        start_carplay_seam(link, state, dongle, carplay);

        crate::shutdown_signal().await;
        println!("[helperd] shutting down");
        livi_runtime::bonjour::stop();
    });
    ExitCode::SUCCESS
}
