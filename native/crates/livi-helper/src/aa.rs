// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(usb_host)]
use livi_aa::usb::Control;
use livi_aa::wpp;
use livi_core_proto::aa_events::{AaHelperEvent, AaTransport};
use livi_core_proto::helper::HelperSockets;
use livi_helper_stack::aa_sock::{self, AaSockDeps};
use livi_helper_stack::livi_sock::Broadcaster;
use livi_helper_stack::sco::ScoSink;
use livi_helper_stack::state::HelperState;
use livi_wifi::{Channel, Security};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(not(target_os = "linux"))]
const AP_WAIT: Duration = Duration::from_secs(15);
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AaConfig {
    pub ssid: String,
    pub passphrase: String,
    pub channel: Channel,
    #[cfg(target_os = "linux")]
    pub wifi_iface: String,
    pub ap_ip: String,
    pub port: u16,
}

pub type Told = (AaConfig, Security, String);

#[derive(Clone, Default)]
pub struct WiredPhones(Arc<Mutex<Vec<String>>>);

impl WiredPhones {
    pub fn set(&self, ids: Vec<String>) {
        *self.0.lock().unwrap() = ids.into_iter().map(|s| s.to_uppercase()).collect();
    }

    fn contains(&self, ids: &[&str]) -> bool {
        let known = self.0.lock().unwrap();
        ids.iter()
            .any(|id| !id.is_empty() && known.contains(&id.to_uppercase()))
    }
}

pub fn on_dongle(base: &AaConfig, passphrase: String) -> Told {
    use livi_link_host::{ap, link};
    let live = livi_link_host::on_air(link::CHOICE);
    let told = AaConfig {
        ssid: live
            .as_ref()
            .map(|ap| ap.ssid.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base.ssid.clone()),
        channel: live.as_ref().map_or(base.channel, |ap| ap.channel),
        ap_ip: ip_facing_dongle().unwrap_or_else(|| base.ap_ip.clone()),
        passphrase,
        ..base.clone()
    };
    (
        told,
        live.map(|ap| ap.security).unwrap_or_default(),
        ap::mac().unwrap_or_default(),
    )
}

fn ip_facing_dongle() -> Option<String> {
    #[cfg(target_os = "linux")]
    let iface = livi_link_host::link::host_iface();
    #[cfg(not(target_os = "linux"))]
    let iface =
        livi_link_host::link::name().and_then(|name| livi_helper_stack::net::iface_facing(&name));
    iface
        .and_then(|iface| livi_helper_stack::net::ipv4_of(&iface))
        .map(|a| a.to_string())
}

#[cfg(not(target_os = "linux"))]
pub async fn watch_dongle(
    mut arrivals: tokio::sync::mpsc::UnboundedReceiver<crate::bt_dongle::Arrival>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    state: Arc<livi_helper_stack::state::HelperState>,
) {
    while let Some(arrival) = arrivals.recv().await {
        let (cfg, bcast, wired, state) = (cfg.clone(), bcast.clone(), wired.clone(), state.clone());
        tokio::spawn(async move {
            let mac = arrival.peer;
            println!("[aa] phone connected mac={mac} over the dongle");
            let up = tokio::task::spawn_blocking(|| livi_link_host::ap::ready(AP_WAIT)).await;
            if !up.unwrap_or(false) {
                eprintln!("[aa] {mac}: the dongle's access point is not up");
                return;
            }
            let told =
                tokio::task::spawn_blocking(move || on_dongle(&cfg, cfg.passphrase.clone())).await;
            let Ok(told) = told else { return };
            bootstrap(arrival.stream, &mac, told, &bcast, &wired, &state).await;
        });
    }
}

#[cfg_attr(not(usb_host), allow(unused_variables))]
pub fn serve(sockets: &HelperSockets, sco_sink: ScoSink, usb: bool) -> (Broadcaster, WiredPhones) {
    let events = Broadcaster::default();
    let wired = WiredPhones::default();
    #[cfg(usb_host)]
    let control = Control::default();
    let deps = AaSockDeps {
        set_wired_phones: Box::new({
            let wired = wired.clone();
            move |ids| wired.set(ids)
        }),
        events: events.clone(),
        set_sco_sink: Box::new(move |target| sco_sink.set(target)),
        #[cfg(usb_host)]
        restart_usb: Box::new({
            let control = control.clone();
            move |serial| control.restart(serial)
        }),
        #[cfg(not(usb_host))]
        restart_usb: Box::new(|_| 0),
    };
    let path = sockets.android_auto.clone();
    tokio::spawn(async move {
        if let Err(e) = aa_sock::serve(&path, deps).await {
            eprintln!("[aa-sock] ended: {e}");
        }
    });
    #[cfg(usb_host)]
    if usb {
        tokio::spawn(watch_usb(control, sockets.sessions.clone(), events.clone()));
        println!("[helper] Android Auto USB watcher started");
    }
    (events, wired)
}

#[cfg(usb_host)]
async fn watch_usb(control: Control, sessions: PathBuf, events: Broadcaster) {
    let subscribed = events.clone();
    livi_aa::usb::run(
        control,
        sessions,
        move |socket, peer, serial| {
            announce_session(&events, socket, peer, AaTransport::Usb, Some(serial))
        },
        async move { subscribed.subscribed().await },
    )
    .await;
}

pub fn stream(fd: OwnedFd) -> std::io::Result<tokio::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(stream)
}

pub async fn bootstrap(
    sock: impl AsyncRead + AsyncWrite + Unpin,
    mac: &str,
    told: Told,
    bcast: &Broadcaster,
    wired: &WiredPhones,
    state: &HelperState,
) {
    state.link_up(mac);
    let ended = handshake(sock, mac, told, bcast, wired).await;
    state.link_down(mac);
    if let Err(e) = ended {
        println!("[aa] {mac}: bootstrap ended: {e}");
    }
}

async fn handshake(
    mut sock: impl AsyncRead + AsyncWrite + Unpin,
    mac: &str,
    told: Told,
    bcast: &Broadcaster,
    wired: &WiredPhones,
) -> std::io::Result<()> {
    let (cfg, security, bssid) = told;
    let (cfg, bssid) = (&cfg, bssid.to_lowercase());
    println!(
        "[aa] {mac}: WPP bootstrap (AP {}:{} ssid={})",
        cfg.ap_ip, cfg.port, cfg.ssid
    );

    sock.write_all(&wpp::wifi_version_request(cfg.channel))
        .await?;

    let mut reader = wpp::FrameReader::default();
    let mut buf = [0u8; 4096];
    let mut pending: Option<(u16, Vec<u8>)> = None;

    if let Some((msg_id, body)) =
        read_frame(&mut sock, &mut reader, &mut buf, VERSION_TIMEOUT).await?
    {
        if msg_id == wpp::MSG_WIFI_VERSION_RESPONSE {
            let id = wpp::parse_identity(&body);
            emit_device(bcast, mac, &id);
            if wired.contains(&[mac, &id.instance_id, &id.serial]) {
                println!("[aa] {mac}: already projecting over USB, not offering the AP");
                return Ok(());
            }
        } else {
            pending = Some((msg_id, body));
        }
    }

    sock.write_all(&wpp::wifi_start_request(&cfg.ap_ip, cfg.port))
        .await?;

    loop {
        let next = match pending.take() {
            Some(f) => Some(f),
            None => read_frame(&mut sock, &mut reader, &mut buf, HANDSHAKE_TIMEOUT).await?,
        };
        let Some((msg_id, body)) = next else {
            println!("[aa] {mac}: WPP handshake timed out");
            return Ok(());
        };

        match msg_id {
            wpp::MSG_WIFI_INFO_REQUEST => {
                let info = wpp::wifi_info_response(&cfg.ssid, &cfg.passphrase, &bssid, security);
                sock.write_all(&info).await?;
            }
            wpp::MSG_WIFI_CONNECT_STATUS => {
                let status = wpp::connect_status(&body);
                if status == 0 {
                    println!("[aa] {mac}: phone joined AP {}", cfg.ssid);
                } else {
                    println!("[aa] {mac}: phone-side AP join failed (status={status})");
                }
            }
            wpp::MSG_WIFI_PING_REQUEST => sock.write_all(&wpp::pong(&body)).await?,
            wpp::MSG_WIFI_VERSION_RESPONSE => {
                let id = wpp::parse_identity(&body);
                emit_device(bcast, mac, &id);
            }
            wpp::MSG_WIFI_START_RESPONSE => {}
            other => println!(
                "[aa] {mac}: unknown WPP message {other} ({} bytes)",
                body.len()
            ),
        }
    }
}

async fn read_frame(
    sock: &mut (impl AsyncRead + Unpin),
    reader: &mut wpp::FrameReader,
    buf: &mut [u8],
    timeout: Duration,
) -> std::io::Result<Option<(u16, Vec<u8>)>> {
    loop {
        if let Some(frame) = reader.next_frame() {
            return Ok(Some(frame));
        }
        let n = match tokio::time::timeout(timeout, sock.read(buf)).await {
            Err(_) => return Ok(None),
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
        };
        reader.push(&buf[..n]);
    }
}

fn emit_device(bcast: &Broadcaster, mac: &str, id: &wpp::Identity) {
    if id.instance_id.is_empty() && id.serial.is_empty() {
        return;
    }
    println!(
        "[aa] {mac}: identified instanceId={} serial={}",
        if id.instance_id.is_empty() {
            "-"
        } else {
            &id.instance_id
        },
        if id.serial.is_empty() {
            "-"
        } else {
            &id.serial
        }
    );
    let event = AaHelperEvent::AaDevice {
        bt_mac: mac.to_uppercase(),
        instance_id: id.instance_id.clone(),
        usb_serial: id.serial.clone(),
    };
    bcast.push_json(event.to_line());
}

pub fn serve_wifi(port: u16, sessions: PathBuf, events: Broadcaster) {
    tokio::spawn(livi_aa::server::run(port, sessions, move |socket, peer| {
        announce_session(&events, socket, peer, AaTransport::Wifi, None);
    }));
}

pub fn announce_session(
    bcast: &Broadcaster,
    socket: &str,
    peer: impl ToString,
    transport: AaTransport,
    serial: Option<&str>,
) {
    let event = AaHelperEvent::AaSession {
        socket: socket.to_string(),
        peer: peer.to_string(),
        transport,
        serial: serial.map(str::to_string),
    };
    bcast.push_json(event.to_line());
}
