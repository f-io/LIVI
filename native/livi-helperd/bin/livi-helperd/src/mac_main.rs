// The dongle's Bluetooth controller is driven from here over its HCI tunnel, since macOS cannot
// lend a foreign controller to its own stack.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use iap2_link::LinkConfig;
use iap2_mfi::{NcmCoprocessor, NoCoprocessor};
use livi_bt_host::sdp::Service;
use livi_runtime::bonjour::Bonjour;
use livi_runtime::bringup::{AskOnAir, CpConfig, run_accessory};
use livi_runtime::driver::spawn_link_stream;
use livi_runtime::hfp::Hfp;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{
    self, Broadcaster, LiviSockConfig, SharedTag, pump_artwork, pump_events_for,
};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::sco::ScoSink;
use livi_runtime::state::HelperState;
use livi_runtime::vehicle::Fuels;
use livi_wifi::Channel;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::bt_dongle::{Arrival, Dongle, Profiles};
use crate::link::LinkPresence;

const AP_WAIT: Duration = Duration::from_secs(15);

static BONJOUR: std::sync::OnceLock<Bonjour> = std::sync::OnceLock::new();

fn env_s(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn accessory_mac(pi: &str) -> [u8; 6] {
    if let Ok(s) = std::env::var("LIVI_CP_BT_MAC") {
        let bytes: Vec<u8> = s.split(':').filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        if bytes.len() == 6 {
            return [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]];
        }
    }
    let mut m = [0x02u8, 0, 0, 0, 0, 0]; // locally-administered
    for (i, b) in pi.bytes().enumerate() {
        m[1 + (i % 5)] ^= b;
    }
    m
}

fn cp_config() -> (CpConfig, Identity) {
    let name = env_s("LIVI_CP_NAME", "LIVI");
    let pi = env_s("LIVI_CP_PI", "");
    let cp = CpConfig {
        ap_mac: None,
        ap_on_air: None,
        wifi_iface: String::new(),
        ssid: name.clone(),
        passphrase: env_s("LIVI_PASSPHRASE", "12345678"),
        channel: Channel::of_number(env_s("LIVI_CHANNEL", "36").parse().unwrap_or(36)),
        airplay_port: env_s("LIVI_CP_AIRPLAY_PORT", "").parse().unwrap_or(0),
        source_version: env_s("LIVI_CP_SOURCE_VERSION", "950.7.1"),
        public_key: pi.clone(),
        transport: Transport::Wired,
        av_iface: None, // resolved per session from the interface facing the dongle
        av_iface_late: None,
        available_current_ma: 500,
        on_cable: None,
        start_again: None,
    };
    let identity = Identity {
        name: name.clone(),
        ssid: name,
        bt_mac: accessory_mac(&pi),
        fuels: Fuels::parse(&env_s("LIVI_CP_FUELS", "")),
    };
    (cp, identity)
}

fn wireless_config(base: &CpConfig) -> CpConfig {
    CpConfig {
        ap_mac: livi_link_host::ap::mac(),
        ssid: livi_link_host::ap::ssid().unwrap_or_else(|| base.ssid.clone()),
        // The phone reaches us over the dongle's link, so that interface carries the session.
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

fn wireless_identity(base: &Identity) -> Identity {
    Identity { bt_mac: livi_link_host::ap::bt_mac().unwrap_or(base.bt_mac), ..base.clone() }
}

async fn wireless_sessions(
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
            println!("[helperd] {} is on the cable, its Bluetooth link goes", session.peer);
            if let Err(e) = dongle.disconnect(&session.peer) {
                eprintln!("[helperd] {} stays on the dongle's bluetooth: {e}", session.peer);
            }
            continue;
        }
        // The phone is about to be told which network to join, so make sure it is on the air.
        if !tokio::task::spawn_blocking(|| livi_link_host::ap::ready(AP_WAIT))
            .await
            .unwrap_or(false)
        {
            eprintln!("[helperd] the dongle's access point is not up, not starting a session");
            continue;
        }
        let cp =
            CpConfig { on_cable: Some(dongle.on_cable(state.clone())), ..wireless_config(&cp) };
        // The phone is talking to the dongle's controller, so that is the address it must hear.
        let identity = Identity { bt_mac: session.local, ..identity.clone() };
        println!(
            "[helperd] phone connected mac={} over the dongle, ap {} channel {}",
            session.peer,
            cp.ap_mac.clone().unwrap_or_default(),
            cp.channel
        );
        let cfg = LinkConfig { max_outgoing: 4, control_version: 2, ..LinkConfig::default() };
        let (channel, art_rx) = spawn_link_stream(session.stream, cfg, false);
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(run_accessory(channel, auth.clone(), identity, cp, tx, state.vehicle_feed()));
        let ident: SharedTag = Default::default();
        tokio::spawn(pump_events_for(rx, bcast.clone(), "bt", None, ident.clone()));
        tokio::spawn(pump_artwork(art_rx, bcast.clone(), ident));
    }
}

fn wireless_android_auto() -> bool {
    env_s("LIVI_AA_WIRELESS", "1") != "0"
}

async fn identify_on_link(link: Arc<LinkPresence>, mut auth: SharedCoprocessor) {
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
                        eprintln!("[helperd] MFi coprocessor not answering yet: {e}");
                        reported = true;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        };
        let Some(major) = major else { continue };
        let kind = if major == 2 { "2.0 (RSA, SHA-1)" } else { "3.0 (ECDSA, SHA-256)" };
        println!("[helperd] MFi coprocessor: auth protocol major {major} — {kind}");
        link.wait_until(false).await;
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
    let auth = SharedCoprocessor::new(Box::new(NoCoprocessor));
    let bcast = Broadcaster::default();

    // A phone that came in over the dongle's Bluetooth carries on wirelessly, so the session on
    // the socket has to identify the same way. Announcing a USB accessory to it gets turned down.
    let over_dongle = over_dongle();

    let (up_auth, down_auth) = (auth.clone(), auth.clone());
    let (arrived, left) = (bcast.clone(), bcast.clone());
    let gone = dongle.clone();
    tokio::spawn(link.clone().resolve(
        move || {
            up_auth.replace(Box::new(NcmCoprocessor::new(&livi_link_host::link::addr(
                livi_net::port::MFI,
            ))));
            arrived.push_json("{\"type\":\"link\",\"up\":true}".into());
        },
        move || {
            down_auth.replace(Box::new(NoCoprocessor));
            // Its links are gone with it, a blocking read would notice only much later.
            gone.end();
            println!("[helperd] the dongle went");
            left.push_json("{\"type\":\"link\",\"up\":false}".into());
        },
    ));
    let sock_cfg = LiviSockConfig {
        path: livi_sock::SOCK_PATH.into(),
        identity: if over_dongle { wireless_identity(&identity) } else { identity.clone() },
        cp: if over_dongle { wireless_config(&cp) } else { cp.clone() },
        // No BlueZ here, our own host on the dongle's controller drops the link.
        disconnect: over_dongle.then(|| {
            let dongle = dongle.clone();
            Arc::new(move |mac: String| dongle.disconnect(&mac)) as _
        }),
        targets: None,
        // The dongle can come back with a new access-point MAC, so it is read per session and
        // the tunnel names the accessory the phone joined.
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
    tokio::spawn(crate::wired::watch_usbmuxd(
        auth,
        identity,
        cp.clone(),
        crate::wired::Dongle {
            ap_mac: dongle_ap.then_some(livi_link_host::ap::mac as fn() -> Option<String>),
            bt_mac: over_dongle.then_some(livi_link_host::ap::bt_mac as fn() -> Option<[u8; 6]>),
        },
        bcast.clone(),
        state.clone(),
        link.clone(),
    ));
    println!("[helperd] wired CarPlay watcher started (system usbmuxd), waiting for the LIVI Link");
    if dongle_ap {
        tokio::spawn(crate::link::relay_stations(bcast.clone()));
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
    let [a, b, c, d, e, f] = accessory_mac(&pi);
    let device_id = format!("{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}");
    match Bonjour::start(
        device_id,
        cp.airplay_port as u16,
        cp.source_version.clone(),
        pk,
        pi,
        bcast,
    ) {
        Ok(b) => {
            // A static is never dropped, so `bonjour::stop` ends its publisher.
            let _ = BONJOUR.set(b);
            println!(
                "[helperd] CarPlay receiver seam ready (cp-bt.sock + bonjour :{})",
                cp.airplay_port
            );
        }
        Err(e) => eprintln!("[helperd] bonjour start failed: {e}"),
    }
}

/// The Wi-Fi bootstrap for the phones the dongle's Bluetooth hands over.
fn start_android_auto(
    arrivals: UnboundedReceiver<Arrival>,
    events: Broadcaster,
    wired: crate::aa::WiredPhones,
    state: Arc<HelperState>,
) {
    let port = env_s("LIVI_PORT", "").parse().unwrap_or(livi_aa::consts::TCP_PORT);
    let sessions = events.clone();
    tokio::spawn(livi_aa::server::run(port, move |socket, peer| {
        sessions.push_json(format!(
            "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"wifi\"}}"
        ));
    }));
    let cfg = crate::aa::AaConfig {
        ssid: env_s("LIVI_CP_NAME", "LIVI"),
        passphrase: env_s("LIVI_PASSPHRASE", "12345678"),
        channel: Channel::of_number(env_s("LIVI_CHANNEL", "36").parse().unwrap_or(36)),
        ap_ip: String::new(),
        port,
    };
    tokio::spawn(crate::aa::watch_dongle(arrivals, cfg, events, wired, state));
    println!("[helperd] wireless Android Auto over the dongle's bluetooth is on");
}

/// Android Auto takes its calls over hands-free on the cable too, wireless only adds the Wi-Fi
/// bootstrap. CarPlay needs Bluetooth only to go wireless.
fn start_bluetooth(
    link: Arc<LinkPresence>,
    state: Arc<HelperState>,
    dongle: Dongle,
    events: Broadcaster,
    wired: crate::aa::WiredPhones,
    sco_sink: ScoSink,
) -> Option<UnboundedReceiver<Arrival>> {
    let wireless_cp = env_s("LIVI_CP_WIRELESS", "") == "1";
    let wireless_aa = wireless_android_auto();
    let (carplay, carplay_arrivals) = unbounded_channel();
    let (android_auto, aa_arrivals) = unbounded_channel();
    let hfp = Hfp::default();
    hfp.set_events(events.clone());
    let mut offered = vec![Service::HandsFree];
    if wireless_cp {
        offered.push(Service::CarPlay);
    }
    if wireless_aa {
        offered.push(Service::AndroidAuto);
        start_android_auto(aa_arrivals, events.clone(), wired, state.clone());
    }
    let profiles = Profiles {
        carplay: wireless_cp.then_some(carplay),
        android_auto: wireless_aa.then_some(android_auto),
        hfp,
        events,
        sco_sink,
    };
    let name = env_s("LIVI_CP_NAME", "LIVI");
    tokio::spawn(crate::bt_dongle::run(dongle, link, profiles, state, offered, name));
    println!("[helperd] hands-free over the dongle's bluetooth is on");
    wireless_cp.then_some(carplay_arrivals)
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
        };
        tokio::spawn(async move {
            let path = livi_runtime::shared_sock::SOCK_PATH;
            if let Err(e) = livi_runtime::shared_sock::serve(path, None, shared).await {
                eprintln!("[shared-sock] ended: {e}");
            }
        });
        let aa_events = Broadcaster::default();
        let usb_control = livi_aa::usb::Control::default();
        let wired_phones = crate::aa::WiredPhones::default();
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
            if let Err(e) = livi_runtime::aa_sock::serve(deps).await {
                eprintln!("[aa-sock] ended: {e}");
            }
        });
        let events = aa_events.clone();
        let subscribed = aa_events.clone();
        tokio::spawn(livi_aa::usb::run(
            usb_control,
            move |socket, peer, serial| {
                events.push_json(format!(
                    "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"usb\",\"serial\":\"{serial}\"}}"
                ));
            },
            async move { subscribed.subscribed().await },
        ));
        println!("[helperd] Android Auto USB watcher started");
        let link = LinkPresence::new();
        let link_state = link.clone();
        tokio::spawn(livi_link_host::run(move |on, _serial| link_state.set_on_bus(on)));
        println!("[helperd] dongle watcher started");

        let state = Arc::new(HelperState::default());
        let dongle = Dongle::default();
        let carplay = if over_dongle() {
            start_bluetooth(link.clone(), state.clone(), dongle.clone(), aa_events, wired_phones, sco_sink)
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
