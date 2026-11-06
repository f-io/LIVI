// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use iap2_mfi::{AuthCoprocessor, NoCoprocessor};
use livi_core_proto::helper::{HelperSockets, HelperStart};
#[cfg(target_os = "macos")]
use livi_runtime::bonjour::Bonjour;
#[cfg(target_os = "macos")]
use livi_runtime::bringup::CpConfig;
use livi_runtime::ident::Transport;
use livi_runtime::livi_sock::{self, Broadcaster, LiviSockConfig};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::state::HelperState;
use tokio::sync::{oneshot, watch};

use crate::link::LinkPresence;
use crate::wired::{Dongle, Wired};

const SHUTDOWN: Duration = Duration::from_secs(2);
#[cfg(target_os = "android")]
const LINK_ANNOUNCE_CHECK: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Host {
    pub sockets: HelperSockets,
    pub mfi: PathBuf,
    pub lockdown: PathBuf,
    #[cfg(target_os = "android")]
    pub tunnel: Arc<iap2_usbmux::Tunnel>,
    #[cfg(target_os = "android")]
    pub phones: crate::wireless::Phones,
    #[cfg(target_os = "android")]
    pub hotspot: crate::wireless::Hotspot,
    #[cfg(target_os = "android")]
    pub drop_phone: livi_sock::DropLink,
    #[cfg(target_os = "android")]
    pub page_phone: crate::wireless::Page,
    pub bt_keys: PathBuf,
}

struct Held {
    _bluez: watch::Sender<Option<livi_sock::Bluez>>,
    #[cfg(target_os = "macos")]
    _bonjour: Option<Bonjour>,
}

pub struct Embedded {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Embedded {
    pub fn start(start: HelperStart, host: Host) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("helper")
            .build()?;
        let (stop, stopped) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("helper".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _held = serve(start, host);
                    let _ = stopped.await;
                });
                runtime.shutdown_timeout(SHUTDOWN);
            })?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(start: HelperStart, host: Host) -> Held {
    let files = crate::mfi::open_in(&host.mfi);
    let own_files = files.is_some();
    if !own_files {
        println!(
            "[helper] no MFi files in {}, CarPlay waits for them",
            host.mfi.display()
        );
    }
    let link = watch_link(&host);
    let over_dongle = start.settings.bt_adapter == livi_link_host::link::CHOICE;
    let dongle_ap = start.settings.wifi_iface == livi_link_host::link::CHOICE;
    let dongle = crate::bt_dongle::Dongle::default();
    let chip: Box<dyn AuthCoprocessor + Send> = match files {
        Some(files) => Box::new(files),
        None => Box::new(NoCoprocessor),
    };
    let auth = SharedCoprocessor::new(chip);
    let (identity, cp) = crate::carplay(&start, Transport::Wired);
    let bcast = Broadcaster::default();
    let state = Arc::new(HelperState::default());
    let (bluez, bluez_later) = watch::channel(None);
    let sco_sink = livi_runtime::sco::ScoSink::default();
    let (aa_events, wired_phones) = crate::aa::serve(&host.sockets, sco_sink.clone(), true);
    #[cfg_attr(not(target_os = "android"), allow(unused_variables))]
    let (carplay, android_auto) = (
        start.settings.wireless_carplay,
        start.settings.wireless_android_auto,
    );
    #[cfg(target_os = "android")]
    let wireless = (!over_dongle && (carplay || android_auto)).then(|| {
        (
            livi_runtime::ident::Identity {
                bt_mac: livi_link_host::accessory::address(&start.device_id)
                    .unwrap_or(identity.bt_mac),
                ..identity.clone()
            },
            livi_runtime::bringup::CpConfig {
                transport: Transport::Wireless,
                ..cp.clone()
            },
        )
    });
    let sock = LiviSockConfig {
        path: host.sockets.carplay.clone(),
        identity: identity.clone(),
        cp: cp.clone(),
        disconnect: None,
        targets: None,
        cp_live: None,
    };
    let sock = if over_dongle {
        let base = cp.clone();
        let dongle = dongle.clone();
        LiviSockConfig {
            identity: crate::dongle::wireless_identity(&identity),
            cp: crate::dongle::wireless_config(&cp),
            disconnect: Some(Arc::new(move |mac: String| dongle.disconnect(&mac))),
            cp_live: Some(Arc::new(move || crate::dongle::wireless_config(&base))),
            ..sock
        }
    } else {
        sock
    };
    #[cfg(target_os = "android")]
    let sock = match &wireless {
        Some((identity, base)) if carplay => {
            let base = base.clone();
            LiviSockConfig {
                identity: identity.clone(),
                disconnect: Some(host.drop_phone.clone()),
                cp_live: Some(Arc::new(move || crate::wireless::session_cp(&base))),
                ..sock
            }
        }
        _ => sock,
    };
    {
        let (auth, bcast, state) = (auth.clone(), bcast.clone(), state.clone());
        tokio::spawn(async move {
            if let Err(e) = livi_sock::serve(sock, auth, bluez_later, bcast, state).await {
                eprintln!("[helper] CarPlay socket ended: {e}");
            }
        });
    }
    crate::dongle::follow_link(
        link.clone(),
        auth.clone(),
        own_files,
        bcast.clone(),
        dongle.clone(),
    );
    tokio::spawn(crate::dongle::identify_on_link(link.clone(), auth.clone()));
    if dongle_ap {
        tokio::spawn(crate::link::relay_stations(bcast.clone()));
    }
    if over_dongle {
        let arrivals = crate::dongle::start_bluetooth(
            radios(&start, &host),
            link.clone(),
            state.clone(),
            dongle.clone(),
            aa_events.clone(),
            wired_phones.clone(),
            sco_sink.clone(),
        );
        if let Some(arrivals) = arrivals {
            tokio::spawn(crate::dongle::wireless_sessions(
                auth.clone(),
                identity.clone(),
                cp.clone(),
                bcast.clone(),
                arrivals,
                state.clone(),
                dongle.clone(),
            ));
            println!("[helper] wireless CarPlay over the dongle's bluetooth is on");
        }
    }
    #[cfg(target_os = "android")]
    let airplay_txt = livi_runtime::bonjour::txt_records(
        &livi_runtime::bringup::accessory_id(&cp).unwrap_or_default(),
        &cp.source_version,
        &start.public_key,
        &start.pairing_id,
    );
    #[cfg(target_os = "android")]
    if over_dongle && carplay {
        tokio::spawn(announce_on_link(
            link.clone(),
            host.tunnel.clone(),
            cp.clone(),
            airplay_txt.clone(),
        ));
    }
    #[cfg(target_os = "android")]
    let shared_bluetooth = wireless.and_then(|(identity, cp)| {
        let android_auto = android_auto.then(|| {
            let sessions = host.sockets.sessions.clone();
            let events = aa_events.clone();
            tokio::spawn(livi_aa::server::run(
                livi_aa::consts::TCP_PORT,
                sessions,
                move |socket, peer| {
                    crate::aa::announce_session(&events, socket, peer, "wifi", None);
                },
            ));
            let hfp = livi_runtime::hfp::Hfp::default();
            hfp.set_events(aa_events.clone());
            crate::wireless::AndroidAuto {
                hfp,
                events: aa_events.clone(),
                wired: wired_phones.clone(),
            }
        });
        let bluetooth = android_auto.as_ref().map(|aa| {
            crate::wireless::bluetooth(
                host.page_phone.clone(),
                host.drop_phone.clone(),
                Some(aa.hfp.clone()),
            )
        });
        tokio::spawn(crate::wireless::listen(
            host.phones.clone(),
            host.hotspot.clone(),
            host.page_phone.clone(),
            crate::wireless::Wireless {
                auth: auth.clone(),
                identity,
                cp,
                bcast: bcast.clone(),
                state: state.clone(),
                airplay_txt: airplay_txt.clone(),
                drop: host.drop_phone.clone(),
                carplay,
                android_auto,
            },
        ));
        bluetooth
    });
    #[cfg(not(target_os = "android"))]
    let shared_bluetooth = None;
    serve_shared(&host.sockets.shared, shared_bluetooth, dongle_ap);
    #[cfg(target_os = "macos")]
    let bonjour = announce(&cp, &start, bcast.clone());
    let wired = Wired {
        auth,
        identity,
        cp,
        dongle: Dongle {
            ap_mac: dongle_ap.then_some(livi_link_host::ap::mac as fn() -> Option<String>),
            bt_mac: over_dongle.then_some(livi_link_host::ap::bt_mac as fn() -> Option<[u8; 6]>),
        },
        bcast,
        state,
        link: if own_files {
            LinkPresence::always()
        } else {
            link
        },
        lockdown: host.lockdown,
        #[cfg(target_os = "android")]
        tunnel: host.tunnel.clone(),
        #[cfg(target_os = "android")]
        airplay_txt,
    };
    #[cfg(target_os = "android")]
    tokio::spawn(crate::wired::watch(wired));
    #[cfg(target_os = "macos")]
    tokio::spawn(crate::wired::watch_usbmuxd(wired));
    Held {
        _bluez: bluez,
        #[cfg(target_os = "macos")]
        _bonjour: bonjour,
    }
}

#[cfg(target_os = "macos")]
fn announce(cp: &CpConfig, start: &HelperStart, bcast: Broadcaster) -> Option<Bonjour> {
    if cp.airplay_port == 0 {
        eprintln!("[helper] LIVI opened no CarPlay port, CarPlay is not announced");
        return None;
    }
    match Bonjour::start(
        livi_runtime::bringup::accessory_id(cp).unwrap_or_default(),
        cp.airplay_port as u16,
        cp.source_version.clone(),
        start.public_key.clone(),
        start.pairing_id.clone(),
        bcast,
    ) {
        Ok(bonjour) => Some(bonjour),
        Err(e) => {
            eprintln!("[helper] bonjour start failed: {e}");
            None
        }
    }
}

#[cfg(target_os = "android")]
async fn announce_on_link(
    link: Arc<LinkPresence>,
    tunnel: Arc<iap2_usbmux::Tunnel>,
    cp: livi_runtime::bringup::CpConfig,
    txt: Vec<String>,
) {
    let mut held: Option<(String, Option<livi_mdns::responder::Responder>)> = None;
    loop {
        let ifname = link.is_present().then(|| tunnel.ifname()).flatten();
        if held.as_ref().map(|(name, _)| name) != ifname.as_ref() {
            held = ifname.map(|name| {
                let responder = crate::wired::announce(&name, tunnel.address(), &cp, &txt);
                (name, responder)
            });
        }
        tokio::time::sleep(LINK_ANNOUNCE_CHECK).await;
    }
}

#[cfg_attr(not(target_os = "android"), allow(unused_variables))]
fn watch_link(host: &Host) -> Arc<LinkPresence> {
    let link = LinkPresence::new();
    #[cfg(target_os = "android")]
    let (tunnel, held) = (host.tunnel.clone(), Arc::default());
    let presence = link.clone();
    tokio::spawn(livi_link_host::run(move |on, _serial| {
        presence.set_on_bus(on);
        #[cfg(target_os = "android")]
        attach_net(on, &tunnel, &held);
    }));
    link
}

#[cfg(target_os = "android")]
fn attach_net(
    on: bool,
    tunnel: &Arc<iap2_usbmux::Tunnel>,
    held: &Arc<std::sync::Mutex<Option<livi_link_host::usb_net::UsbNet>>>,
) {
    if !on {
        held.lock().unwrap().take();
        return;
    }
    let (tunnel, held) = (tunnel.clone(), held.clone());
    tokio::task::spawn_blocking(move || match livi_link_host::usb_net::attach(tunnel) {
        Ok(net) => *held.lock().unwrap() = Some(net),
        Err(e) => eprintln!("[link] {e}"),
    });
}

fn radios(start: &HelperStart, host: &Host) -> crate::dongle::Radios {
    let s = &start.settings;
    let channel = if s.channel == 0 {
        crate::DEFAULT_CHANNEL
    } else {
        s.channel
    };
    crate::dongle::Radios {
        name: crate::or(&s.car_name, crate::DEFAULT_NAME),
        passphrase: crate::or(&s.passphrase, crate::DEFAULT_PASSPHRASE),
        channel: livi_wifi::Channel::of_number(channel),
        port: livi_aa::consts::TCP_PORT,
        carplay: s.wireless_carplay,
        android_auto: s.wireless_android_auto,
        sockets: host.sockets.clone(),
        keys: host.bt_keys.clone(),
    }
}

fn serve_shared(
    path: &std::path::Path,
    bluetooth: Option<livi_runtime::shared_sock::Bluetooth>,
    dongle_ap: bool,
) {
    let deps = livi_runtime::shared_sock::SharedSockDeps {
        adapter: String::new(),
        wifi_iface: String::new(),
        events: Broadcaster::default(),
        set_playback_status: Box::new(|_| {}),
        deauth_dongle: dongle_ap.then_some(livi_link_host::ap::deauth as fn() -> Option<usize>),
        bluetooth,
    };
    let path = path.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = livi_runtime::shared_sock::serve(&path, None, deps).await {
            eprintln!("[helper] shared socket ended: {e}");
        }
    });
}
