use std::process::ExitCode;

use iap2_link::LinkConfig;
use iap2_mfi::{I2cCoprocessor, NcmCoprocessor, NoCoprocessor};
use std::sync::Arc;

use livi_runtime::bonjour::Bonjour;
use livi_runtime::bringup::{AskOnAir, CpConfig, run_accessory};
use livi_runtime::bt;
use livi_runtime::driver::{spawn_link, spawn_link_stream};
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{
    self, Bluez, Broadcaster, LiviSockConfig, SharedTag, pump_artwork, pump_events_for,
};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::reconnect;
use livi_runtime::state::HelperState;
use livi_runtime::vehicle::Fuels;
use livi_wifi::{Band, Channel};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn config_path() -> std::path::PathBuf {
    let user = std::env::var("SUDO_USER")
        .ok()
        .filter(|u| !u.is_empty() && u != "root")
        .or_else(|| std::env::var("USER").ok());
    let home = match user {
        Some(u) if u != "root" => format!("/home/{u}"),
        _ => std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
    };
    std::path::Path::new(&home).join(".config/LIVI/config.json")
}

pub struct DeviceConfig {
    json: serde_json::Value,
}

impl DeviceConfig {
    pub fn load() -> Self {
        let json = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        Self { json }
    }

    pub fn string(&self, json_key: &str, env_key: &str, default: &str) -> String {
        if let Some(s) = self.json.get(json_key).and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return s.to_string();
        }
        std::env::var(env_key).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
    }

    pub fn int<T: std::str::FromStr + std::convert::TryFrom<i64>>(
        &self,
        json_key: &str,
        env_key: &str,
        default: T,
    ) -> T {
        if let Some(n) = self.json.get(json_key).and_then(|v| v.as_i64())
            && let Ok(v) = T::try_from(n)
        {
            return v;
        }
        env_or(env_key, default)
    }
}

/// The channel number alone is ambiguous once 6 GHz is picked.
fn configured_channel(dc: &DeviceConfig) -> Channel {
    let band = Band::of_setting(&dc.string("wifiType", "LIVI_WIFI_TYPE", "5ghz"));
    Channel::of_setting(band.unwrap_or(Band::Ghz5), dc.int("wifiChannel", "LIVI_CHANNEL", 36u32))
}

fn ap_iface(dc: &DeviceConfig) -> String {
    let iface = dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0");
    if iface != livi_link_host::link::CHOICE {
        return iface;
    }
    livi_link_host::link::host_iface().unwrap_or(iface)
}

/// Only the chosen controller may be made discoverable, so the helper waits for the dongle's.
async fn bt_adapter(dc: &DeviceConfig) -> String {
    let adapter = dc.string("btAdapter", "LIVI_BT_ADAPTER", "hci0");
    if adapter != livi_link_host::link::CHOICE {
        return adapter;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ours = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lost = ours.clone();
    livi_link_host::bt::attach(
        move |index| {
            ours.store(true, std::sync::atomic::Ordering::Relaxed);
            if tx.send(index).is_err() {
                eprintln!("[bt] the dongle's controller came back, starting over with it");
                std::process::exit(1);
            }
        },
        move || {
            if lost.load(std::sync::atomic::Ordering::Relaxed) {
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

pub fn run_wifi_ap() -> ExitCode {
    let dc = DeviceConfig::load();
    let cfg = livi_runtime::wifi_ap::ApConfig {
        iface: ap_iface(&dc),
        ssid: dc.string("carName", "LIVI_CP_NAME", "LIVI"),
        passphrase: dc.string("wifiPassword", "LIVI_PASSPHRASE", "12345678"),
        channel: configured_channel(&dc),
        width: dc.int("wifiChannelWidth", "LIVI_CHANNEL_WIDTH", 40u32),
        country: dc.string("country", "LIVI_COUNTRY", "DE"),
        ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
    };
    livi_runtime::wifi_ap::run(cfg)
}

pub fn run_wifi_ap_status() -> ExitCode {
    let dc = DeviceConfig::load();
    print!("{}", livi_runtime::wifi_ap::status(&ap_iface(&dc)));
    ExitCode::SUCCESS
}

fn installed(what: &str, result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[{what}] install failed: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn run_install_wifi_ap(unit: Option<String>, rule: Option<String>) -> ExitCode {
    let (Some(unit), Some(rule)) = (unit, rule) else {
        eprintln!("[wifi-ap] usage: --install-wifi-ap <unit file> <sudoers file>");
        return ExitCode::FAILURE;
    };
    installed("wifi-ap", livi_runtime::privileged::install_wifi_ap(&unit, &rule))
}

pub fn run_install_udev_rule(rule: Option<String>, filter: Option<String>) -> ExitCode {
    let Some(rule) = rule else {
        eprintln!("[udev] usage: --install-udev-rule <rule file> [<touch filter>]");
        return ExitCode::FAILURE;
    };
    installed("udev", livi_runtime::privileged::install_udev_rule(&rule, filter.as_deref()))
}

pub fn run_install_gvfs_guard(script: Option<String>, rule: Option<String>) -> ExitCode {
    let (Some(script), Some(rule)) = (script, rule) else {
        eprintln!("[gvfs] usage: --install-gvfs-guard <script file> <sudoers file>");
        return ExitCode::FAILURE;
    };
    installed("gvfs", livi_runtime::privileged::install_gvfs_guard(&script, &rule))
}

pub fn run_wifi_ap_claim() -> ExitCode {
    let dc = DeviceConfig::load();
    livi_runtime::wifi_ap::release_iface_from_nm(&ap_iface(&dc));
    ExitCode::SUCCESS
}

pub fn run_wifi_ap_teardown() -> ExitCode {
    let iface = livi_runtime::wifi_ap::unmanaged_iface().unwrap_or_else(|| {
        DeviceConfig::load().string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0")
    });
    livi_runtime::wifi_ap::teardown(&iface);
    ExitCode::SUCCESS
}

pub fn run_bt_tunnel() -> ExitCode {
    match livi_link_host::bt::tunnel(&|_| {}) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[bt] {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn run() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let served = rt.block_on(serve());
    rt.shutdown_timeout(std::time::Duration::from_secs(1));
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[helperd] error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let dc = DeviceConfig::load();
    let bus_num: u32 = dc.int("carPlayMfiI2cBus", "LIVI_CP_MFI_I2C_BUS", 2);
    let gpio: i32 = dc.int("carPlayMfiPowerGpio", "LIVI_CP_MFI_POWER_GPIO", 21);
    let name = dc.string("carName", "LIVI_CP_NAME", "LIVI");
    let ssid = name.clone();
    let wifi_iface = ap_iface(&dc);
    let dongle_ap =
        dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0") == livi_link_host::link::CHOICE;
    let ap_mac = dongle_ap.then(livi_link_host::ap::mac).flatten();
    let cp = CpConfig {
        wifi_iface: wifi_iface.clone(),
        ssid: ssid.clone(),
        passphrase: dc.string("wifiPassword", "LIVI_PASSPHRASE", "12345678"),
        channel: configured_channel(&dc),
        airplay_port: env_or("LIVI_CP_AIRPLAY_PORT", 0),
        source_version: dc.string("carPlaySourceVersion", "LIVI_CP_SOURCE_VERSION", "950.7.1"),
        public_key: std::env::var("LIVI_CP_PI").unwrap_or_default(),
        transport: Transport::Wireless,
        av_iface: None,
        av_iface_late: None,
        available_current_ma: dc.int(
            "carPlayAvailableCurrentMa",
            "LIVI_CP_AVAILABLE_CURRENT_MA",
            500u16,
        ),
        ap_mac: ap_mac.clone(),
        ap_on_air: dongle_ap.then_some(livi_link_host::ap::on_air as AskOnAir),
        on_cable: None,
        start_again: None,
    };
    let pk = std::env::var("LIVI_CP_PK").unwrap_or_default();
    let pi = std::env::var("LIVI_CP_PI").unwrap_or_default();

    println!("[helperd] opening MFi bus={bus_num} gpio={gpio}");
    let (auth, mfi_link) = match I2cCoprocessor::open(bus_num, gpio) {
        Ok(chip) => {
            println!("[helperd] MFi addr=0x{:02X}", chip.address());
            (SharedCoprocessor::new(Box::new(chip)), crate::link::LinkPresence::always())
        }
        Err(e) => {
            println!(
                "[helperd] no local MFi ({e}); a LIVI Link dongle's chip serves once on the bus"
            );
            let auth = SharedCoprocessor::new(Box::new(NoCoprocessor));
            let link = crate::link::LinkPresence::new();
            let (up_auth, down_auth) = (auth.clone(), auth.clone());
            tokio::spawn(link.clone().resolve(
                move || {
                    up_auth.replace(Box::new(NcmCoprocessor::new(&livi_link_host::link::addr(
                        livi_net::port::MFI,
                    ))))
                },
                move || down_auth.replace(Box::new(NoCoprocessor)),
            ));
            (auth, link)
        }
    };

    let bcast = Broadcaster::default();
    let aa_events = Broadcaster::default();
    let state = Arc::new(HelperState::default());
    let wired_phones = crate::aa::WiredPhones::default();
    let usb_control = livi_aa::usb::Control::default();
    let sco_sink = livi_runtime::sco::ScoSink::default();

    let fuels = Fuels::parse(&std::env::var("LIVI_CP_FUELS").unwrap_or_default());
    let identity = Identity { name, ssid, bt_mac: [0; 6], fuels };
    let (bluez, bluez_later) = tokio::sync::watch::channel(None);
    let sock_cfg = LiviSockConfig {
        path: livi_sock::SOCK_PATH.into(),
        identity: identity.clone(),
        cp: cp.clone(),
        disconnect: None,
        targets: None,
        cp_live: dongle_ap.then(|| {
            let base = cp.clone();
            Arc::new(move || session_cp(&base, true)) as _
        }),
    };
    {
        let bcast = bcast.clone();
        let state = state.clone();
        let auth = auth.clone();
        tokio::spawn(async move {
            if let Err(e) = livi_sock::serve(sock_cfg, auth, bluez_later, bcast, state).await {
                eprintln!("[helperd] livi_sock ended: {e}");
            }
        });
    }
    if std::env::var("LIVI_DONGLE").unwrap_or_else(|_| "1".into()) != "0" {
        let mfi_link_state = mfi_link.clone();
        tokio::spawn(livi_link_host::run(move |on, _serial| mfi_link_state.set_on_bus(on)));
        println!("[helperd] dongle watcher started");
    }
    if std::env::var("LIVI_CP_WIRED").unwrap_or_else(|_| "1".into()) != "0" {
        let wired_cp = CpConfig { transport: Transport::Wired, av_iface: None, ..cp.clone() };
        tokio::spawn(crate::wired::watch(
            auth.clone(),
            identity.clone(),
            wired_cp,
            crate::wired::Dongle {
                ap_mac: dongle_ap.then_some(livi_link_host::ap::mac as fn() -> Option<String>),
                bt_mac: None,
            },
            bcast.clone(),
            state.clone(),
            mfi_link.clone(),
        ));
        println!("[helperd] wired CarPlay watcher started");
    }
    if std::env::var("LIVI_AA_USB").unwrap_or_else(|_| "1".into()) != "0" {
        let events = aa_events.clone();
        let subscribed = aa_events.clone();
        tokio::spawn(livi_aa::usb::run(
            usb_control.clone(),
            move |socket, peer, serial| {
                events.push_json(format!(
                    "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"usb\",\"serial\":\"{serial}\"}}"
                ));
            },
            async move { subscribed.subscribed().await },
        ));
        println!("[helperd] Android Auto USB watcher started");
    }
    {
        let wired = wired_phones.clone();
        let deps = livi_runtime::aa_sock::AaSockDeps {
            set_wired_phones: Box::new(move |ids| wired.set(ids)),
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
            if let Err(e) = livi_runtime::aa_sock::serve(deps).await {
                eprintln!("[aa-sock] ended: {e}");
            }
        });
    }

    let bluetooth = async || -> Result<(), Box<dyn std::error::Error>> {
        let adapter = bt_adapter(&dc).await;
        livi_runtime::bluetoothd::setup();
        println!("[helperd] starting BlueZ profile on {adapter}");
        let (conn, mut incoming, held) = bt::start(&adapter, &identity.name, true).await?;
        // Off unless asked for, since the tunnelled adapter wants the same controller.
        let mut dongle_iap = std::env::var("LIVI_BT_VIA_DONGLE")
            .is_ok_and(|v| v == "1")
            .then(|| livi_link_host::accessory::sessions(|| true));
        let bt_mac = bt::adapter_address(&conn, &adapter).await?;
        println!("[helperd] adapter {} up (RFCOMM ch {})", format_mac(&bt_mac), bt::IAP_CHANNEL);
        let identity = Identity { bt_mac, ..identity.clone() };
        let _ = bluez.send(Some(Bluez { bus: conn.clone(), adapter: adapter.clone(), bt_mac }));

        tokio::spawn(reconnect::run(
            conn.clone(),
            adapter.clone(),
            ap_iface(&DeviceConfig::load()),
            state.clone(),
        ));

        // Android Auto takes its calls over hands-free on the cable too, the switch below only
        // adds the Wi-Fi bootstrap.
        let hfp = livi_runtime::hfp::Hfp::default();
        hfp.set_events(aa_events.clone());
        if let Err(e) = bt::start_hfp(&conn, &adapter, hfp).await {
            eprintln!("[hfp] profile registration failed: {e}");
        }
        livi_runtime::sco::serve(aa_events.clone(), sco_sink.clone());

        if std::env::var("LIVI_AA_WIRELESS").unwrap_or_else(|_| "1".into()) != "0" {
            let aa_port = env_or("LIVI_PORT", livi_aa::consts::TCP_PORT);
            let events = aa_events.clone();
            tokio::spawn(livi_aa::server::run(aa_port, move |socket, peer| {
                events.push_json(format!(
                "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"wifi\"}}"
            ));
            }));
            match bt::start_aa(&conn, &adapter).await {
                Ok(incoming) => {
                    let aa_cfg = crate::aa::AaConfig {
                        ssid: cp.ssid.clone(),
                        passphrase: cp.passphrase.clone(),
                        channel: cp.channel,
                        wifi_iface: wifi_iface.clone(),
                        ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
                        port: aa_port,
                    };
                    if let Err(e) = bt::start_ble_ad(&conn, &adapter, &identity.name).await {
                        eprintln!("[aa] BLE advertisement failed: {e}");
                    }
                    tokio::spawn(crate::aa::watch(
                        incoming,
                        aa_cfg,
                        aa_events.clone(),
                        wired_phones.clone(),
                        state.clone(),
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
            let deps = livi_runtime::shared_sock::SharedSockDeps {
                adapter: adapter.clone(),
                wifi_iface: wifi_iface.clone(),
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
                deauth_dongle: dongle_ap
                    .then_some(livi_link_host::ap::deauth as fn() -> Option<usize>),
            };
            tokio::spawn(async move {
                let path = livi_runtime::shared_sock::SOCK_PATH;
                if let Err(e) = livi_runtime::shared_sock::serve(path, Some(bus), deps).await {
                    eprintln!("[shared-sock] ended: {e}");
                }
            });
        }

        if dongle_ap {
            tokio::spawn(crate::link::relay_stations(bcast.clone()));
        }

        let device_id =
            livi_runtime::bringup::accessory_id(&cp).unwrap_or_else(|| format_mac(&bt_mac));
        let _bonjour = if cp.airplay_port == 0 {
            eprintln!("[helperd] LIVI opened no CarPlay port, CarPlay is not announced");
            None
        } else {
            match Bonjour::start(
                device_id,
                cp.airplay_port as u16,
                cp.source_version.clone(),
                pk,
                pi,
                bcast.clone(),
            ) {
                Ok(b) => Some(b),
                Err(e) => {
                    eprintln!("[helperd] bonjour start failed: {e}");
                    None
                }
            }
        };

        let bus = conn.clone();
        loop {
            tokio::select! {
                _ = crate::shutdown_signal() => {
                    println!("[helperd] shutting down");
                    bt::let_go(&conn, &adapter, &held).await;
                    iap2_usbmux::restore_all_default_config();
                    return Ok(());
                }
                session = async { dongle_iap.as_mut().unwrap().recv().await }, if dongle_iap.is_some() => {
                    let Some(session) = session else { return Ok(()) };
                    if state.carkit_claims(&session.peer) {
                        println!("[helperd] {} is on the cable, its Bluetooth link goes", session.peer);
                        crate::link::drop_dongle_link(session.peer.to_string());
                        continue;
                    }
                    let auth = auth.clone();
                    println!("[helperd] phone connected mac={}", session.peer);
                    let cfg = LinkConfig { max_outgoing: 4, control_version: 2, ..LinkConfig::default() };
                    let (channel, art_rx) = spawn_link_stream(session.stream, cfg, false);
                    let (tx, rx) = tokio::sync::mpsc::channel(64);
                    let cp = CpConfig { on_cable: Some(crate::link::dongle_on_cable(state.clone())), ..session_cp(&cp, dongle_ap) };
                    let (accessory, mac) = (run_accessory(channel, auth, identity.clone(), cp, tx, state.vehicle_feed()), session.peer.to_string());
                    let links = state.clone();
                    tokio::spawn(async move {
                        links.link_up(&mac);
                        accessory.await;
                        links.link_down(&mac);
                    });
                    let ident: SharedTag = Default::default();
                    tokio::spawn(pump_events_for(rx, bcast.clone(), "bt", None, ident.clone()));
                    tokio::spawn(pump_artwork(art_rx, bcast.clone(), ident));
                }
                conn = incoming.recv() => {
                    let Some(conn) = conn else { return Ok(()) };
                    if state.carkit_claims(&conn.peer_mac) {
                        println!("[helperd] {} is on the cable, its Bluetooth link goes", conn.peer_mac);
                        bt::drop_link(&bus, &adapter, conn.peer_mac.clone());
                        continue;
                    }
                    let auth = auth.clone();
                    println!("[helperd] phone connected mac={}", conn.peer_mac);
                    let cfg = LinkConfig { max_outgoing: 4, control_version: 2, ..LinkConfig::default() };
                    let (channel, art_rx) = spawn_link(conn.fd, cfg, false);
                    let (tx, rx) = tokio::sync::mpsc::channel(64);
                    let cp = CpConfig { on_cable: Some(bt::on_cable(state.clone(), &bus, &adapter)), ..session_cp(&cp, dongle_ap) };
                    let (accessory, mac) = (run_accessory(channel, auth, identity.clone(), cp, tx, state.vehicle_feed()), conn.peer_mac.clone());
                    let links = state.clone();
                    tokio::spawn(async move {
                        links.link_up(&mac);
                        accessory.await;
                        links.link_down(&mac);
                    });
                    let ident: SharedTag = Default::default();
                    tokio::spawn(pump_events_for(rx, bcast.clone(), "bt", None, ident.clone()));
                    tokio::spawn(pump_artwork(art_rx, bcast.clone(), ident));
                }
            }
        }
    };

    if let Err(e) = bluetooth().await {
        eprintln!(
            "[helperd] no Bluetooth ({e}), wireless Android Auto and calls are off, wired Android Auto and CarPlay carry on"
        );
        crate::shutdown_signal().await;
        println!("[helperd] shutting down");
        iap2_usbmux::restore_all_default_config();
    }
    Ok(())
}

/// The dongle's access-point MAC can change, so it is read for every session.
fn session_cp(cp: &CpConfig, dongle_ap: bool) -> CpConfig {
    if !dongle_ap {
        return cp.clone();
    }
    CpConfig { ap_mac: livi_link_host::ap::mac().or_else(|| cp.ap_mac.clone()), ..cp.clone() }
}

fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}
