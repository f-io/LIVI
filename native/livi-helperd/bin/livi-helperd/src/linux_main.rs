use std::process::ExitCode;

use iap2_csm::messages::wifi::SecurityType;
use iap2_link::LinkConfig;
use iap2_mfi::{I2cCoprocessor, NcmCoprocessor, NoCoprocessor};
use std::sync::Arc;

use livi_runtime::bonjour::Bonjour;
use livi_runtime::bringup::{run_accessory, CpConfig};
use livi_runtime::bt;
use livi_runtime::driver::spawn_link;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{self, pump_artwork, pump_events_for, Broadcaster, LiviSockConfig, SharedTag};
use livi_runtime::mfi_async::{NoAuth, SharedCoprocessor};
use livi_runtime::reconnect;
use livi_runtime::state::HelperState;

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

    // config.json wins, then env, then default. Mirrors the previous helper.
    pub fn string(&self, json_key: &str, env_key: &str, default: &str) -> String {
        if let Some(s) = self.json.get(json_key).and_then(|v| v.as_str())
            && !s.is_empty() {
                return s.to_string();
            }
        std::env::var(env_key).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
    }

    pub fn int<T: std::str::FromStr + std::convert::TryFrom<i64>>(&self, json_key: &str, env_key: &str, default: T) -> T {
        if let Some(n) = self.json.get(json_key).and_then(|v| v.as_i64())
            && let Ok(v) = T::try_from(n) {
                return v;
            }
        env_or(env_key, default)
    }
}

/// `--wifi-ap`: dedicated early-boot AP mode (hostapd + dnsmasq ownership).
pub fn run_wifi_ap() -> ExitCode {
    let dc = DeviceConfig::load();
    let cfg = livi_runtime::wifi_ap::ApConfig {
        iface: dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0"),
        ssid: dc.string("carName", "LIVI_CP_NAME", "LIVI"),
        passphrase: dc.string("wifiPassword", "LIVI_PASSPHRASE", "12345678"),
        channel: dc.int("wifiChannel", "LIVI_CHANNEL", 36u16) as u8,
        width: dc.int("wifiChannelWidth", "LIVI_CHANNEL_WIDTH", 40u16) as u8,
        country: dc.string("country", "LIVI_COUNTRY", "DE"),
        ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
    };
    livi_runtime::wifi_ap::run(cfg)
}

pub fn run_wifi_ap_teardown() -> ExitCode {
    let dc = DeviceConfig::load();
    let iface = dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0");
    livi_runtime::wifi_ap::teardown(&iface);
    ExitCode::SUCCESS
}

pub fn run() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(serve()) {
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
    let adapter = dc.string("btAdapter", "LIVI_BT_ADAPTER", "hci0");
    let name = dc.string("carName", "LIVI_CP_NAME", "LIVI");
    let ssid = name.clone();
    let wifi_iface = dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0");
    let cp = CpConfig {
        wifi_iface: wifi_iface.clone(),
        ssid: ssid.clone(),
        passphrase: dc.string("wifiPassword", "LIVI_PASSPHRASE", "12345678"),
        channel: dc.int("wifiChannel", "LIVI_CHANNEL", 36u16) as u8,
        security_type: SecurityType::WpaWpa2,
        airplay_port: env_or("LIVI_CP_AIRPLAY_PORT", 7000),
        source_version: dc.string("carPlaySourceVersion", "LIVI_CP_SOURCE_VERSION", "950.7.1"),
        public_key: std::env::var("LIVI_CP_PI").unwrap_or_default(),
        transport: Transport::Wireless,
        av_iface: None,
        available_current_ma: dc.int("carPlayAvailableCurrentMa", "LIVI_CP_AVAILABLE_CURRENT_MA", 500u16),
    };
    let pk = std::env::var("LIVI_CP_PK").unwrap_or_default();
    let pi = std::env::var("LIVI_CP_PI").unwrap_or_default();

    // MFi backend: the local i2c coprocessor, else the chip of a LIVI Link dongle while it is
    // on the bus.
    println!("[helperd] opening MFi bus={bus_num} gpio={gpio}");
    let (auth, mfi_link) = match I2cCoprocessor::open(bus_num, gpio) {
        Ok(chip) => {
            println!("[helperd] MFi addr=0x{:02X}", chip.address());
            (Some(SharedCoprocessor::new(Box::new(chip))), crate::link::LinkPresence::always())
        }
        Err(e) => {
            println!("[helperd] no local MFi ({e}); a LIVI Link dongle's chip serves once on the bus");
            let auth = SharedCoprocessor::new(Box::new(NoCoprocessor));
            let link = crate::link::LinkPresence::new();
            let (up_auth, down_auth) = (auth.clone(), auth.clone());
            tokio::spawn(link.clone().resolve(
                move || {
                    up_auth.replace(Box::new(NcmCoprocessor::new(&livi_dongle::link::addr(
                        iap2_mfi::ncm::DEFAULT_PORT,
                    ))))
                },
                move || down_auth.replace(Box::new(NoCoprocessor)),
            ));
            (Some(auth), link)
        }
    };

    let bcast = Broadcaster::default();
    let aa_events = Broadcaster::default();
    let state = Arc::new(HelperState::default());
    let wired_phones = crate::aa::WiredPhones::default();
    let sco_sink = livi_runtime::sco::ScoSink::default();

    livi_runtime::bluetoothd::setup();
    println!("[helperd] starting BlueZ profile on {adapter}");
    let (conn, mut incoming) = bt::start(&adapter, &name, true).await?;
    let bt_mac = bt::adapter_address(&conn, &adapter).await?;
    println!("[helperd] adapter {} up (RFCOMM ch {})", format_mac(&bt_mac), bt::IAP_CHANNEL);

    let identity = Identity { name, ssid, bt_mac };

    let sock_cfg = LiviSockConfig {
        path: livi_sock::SOCK_PATH.into(),
        adapter: adapter.clone(),
        identity: identity.clone(),
        cp: cp.clone(),
    };
    {
        let bus = conn.clone();
        let bcast = bcast.clone();
        let state = state.clone();
        // Also serves subscribe/reconnect/disconnect, so it runs without MFi.
        match auth.clone() {
            Some(auth) => {
                tokio::spawn(async move {
                    if let Err(e) = livi_sock::serve(sock_cfg, auth, Some(bus), bcast, state).await {
                        eprintln!("[helperd] livi_sock ended: {e}");
                    }
                });
            }
            None => {
                tokio::spawn(async move {
                    if let Err(e) = livi_sock::serve(sock_cfg, NoAuth, Some(bus), bcast, state).await {
                        eprintln!("[helperd] livi_sock ended: {e}");
                    }
                });
            }
        }
    }

    tokio::spawn(reconnect::run(conn.clone(), adapter.clone(), state.clone()));

    if std::env::var("LIVI_AA_WIRELESS").unwrap_or_else(|_| "1".into()) != "0" {
        // The projection listener the WPP bootstrap points the phone at.
        let events = aa_events.clone();
        tokio::spawn(livi_aa::server::run(env_or("LIVI_PORT", 5277u16), move |socket, peer| {
            events.push_json(format!(
                "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"wifi\"}}"
            ));
        }));
        match bt::start_aa(&conn).await {
            Ok(incoming) => {
                let aa_cfg = crate::aa::AaConfig {
                    ssid: cp.ssid.clone(),
                    passphrase: cp.passphrase.clone(),
                    channel: cp.channel as u16,
                    wifi_iface: wifi_iface.clone(),
                    ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
                    port: env_or("LIVI_PORT", 5277u16),
                };
                let hfp = livi_runtime::hfp::Hfp::default();
                hfp.set_events(aa_events.clone());
                if let Err(e) = bt::start_hfp(&conn, hfp.clone()).await {
                    eprintln!("[hfp] profile registration failed: {e}");
                }
                livi_runtime::sco::serve(aa_events.clone(), sco_sink.clone());
                if let Err(e) = bt::start_ble_ad(&conn, &adapter, &identity.name).await {
                    eprintln!("[aa] BLE advertisement failed: {e}");
                }
                tokio::spawn(crate::aa::watch(incoming, aa_cfg, aa_events.clone(), wired_phones.clone(), hfp));
            }
            Err(e) => eprintln!("[aa] profile registration failed: {e}"),
        }
    }

    if std::env::var("LIVI_AA_USB").unwrap_or_else(|_| "1".into()) != "0" {
        // Phones on USB are switched to accessory mode and served here as well.
        let events = aa_events.clone();
        tokio::spawn(livi_aa::usb::run(move |socket, peer, serial| {
            events.push_json(format!(
                "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"usb\",\"serial\":\"{serial}\"}}"
            ));
        }));
        println!("[helperd] Android Auto USB watcher started");
    }
    if std::env::var("LIVI_DONGLE").unwrap_or_else(|_| "1".into()) != "0" {
        let events = aa_events.clone();
        let mfi_link_state = mfi_link.clone();
        tokio::spawn(livi_dongle::run(move |on, _serial| mfi_link_state.set_on_bus(on), move |socket, a| {
            events.push_json(
                serde_json::json!({
                    "event": "dongle-session", "socket": socket, "serial": a.serial,
                    "product": a.product, "version": a.version, "name": a.name
                })
                .to_string(),
            );
        }));
        println!("[helperd] dongle watcher started");
    }
    let mpris = match bt::start_media_player(&conn, &adapter, aa_events.clone()).await {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("[aa] media player failed: {e}");
            None
        }
    };

    {
        let bus = conn.clone();
        let wired = wired_phones.clone();
        let deps = livi_runtime::aa_sock::AaSockDeps {
            adapter: adapter.clone(),
            wifi_iface: wifi_iface.clone(),
            set_wired_phones: Box::new(move |ids| wired.set(ids)),
            events: aa_events.clone(),
            set_sco_sink: Box::new({
                let sink = sco_sink.clone();
                move |target| sink.set(target)
            }),
            set_playback_status: Box::new(move |state| {
                let Some(h) = mpris.clone() else { return };
                let status = match state {
                    "playing" => "Playing",
                    "paused" => "Paused",
                    _ => "Stopped",
                };
                tokio::spawn(async move { h.set_status(status).await });
            }),
        };
        tokio::spawn(async move {
            if let Err(e) = livi_runtime::aa_sock::serve(Some(bus), deps).await {
                eprintln!("[aa-sock] ended: {e}");
            }
        });
    }

    if let Some(auth) = auth.clone()
        && std::env::var("LIVI_CP_WIRED").unwrap_or_else(|_| "1".into()) != "0" {
            let wired_cp = CpConfig {
                transport: Transport::Wired,
                av_iface: None,
                ..cp.clone()
            };
            tokio::spawn(crate::wired::watch(
                auth,
                identity.clone(),
                wired_cp,
                bcast.clone(),
                state.clone(),
                mfi_link.clone(),
            ));
            println!("[helperd] wired CarPlay watcher started");
        }

    let wlan_mac = livi_runtime::net::wlan_mac(&wifi_iface).unwrap_or_else(|| format_mac(&bt_mac));
    let _bonjour = match Bonjour::start(wlan_mac, cp.airplay_port as u16, cp.source_version.clone(), pk, pi, bcast.clone()) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("[helperd] bonjour start failed: {e}");
            None
        }
    };

    loop {
        tokio::select! {
            _ = shutdown_signal() => {
                println!("[helperd] shutting down");
                bt::set_discoverable(&conn, &adapter, false).await;
                iap2_usbmux::restore_all_default_config();
                return Ok(());
            }
            conn = incoming.recv() => {
                let Some(conn) = conn else { return Ok(()) };
                let Some(auth) = auth.clone() else {
                    println!("[helperd] BT phone connected but MFi off; CarPlay link ignored");
                    continue;
                };
                println!("[helperd] phone connected mac={}", conn.peer_mac);
                let cfg = LinkConfig { max_outgoing: 4, control_version: 2, ..LinkConfig::default() };
                let (channel, art_rx) = spawn_link(conn.fd, cfg, false);
                let (tx, rx) = tokio::sync::mpsc::channel(64);
                tokio::spawn(run_accessory(channel, auth, identity.clone(), cp.clone(), tx));
                let ident: SharedTag = Default::default();
                tokio::spawn(pump_events_for(rx, bcast.clone(), "bt", None, ident.clone()));
                tokio::spawn(pump_artwork(art_rx, bcast.clone(), ident));
            }
        }
    }
}

/// Ctrl-C or the TERM systemd/Electron sends when LIVI stops.
async fn shutdown_signal() {
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}
