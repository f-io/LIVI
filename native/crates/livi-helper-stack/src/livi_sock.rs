// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::net::UnixStream;
use tokio::sync::{Notify, mpsc, watch};

use iap2_link::LinkConfig;
use livi_core_proto::helper_events::{HelperKind, NowPlayingPatch};
use livi_core_proto::helper_requests::CpRequest;

use crate::bringup::{BringupEvent, CpConfig, run_accessory};
use crate::driver::{ArtworkRx, LinkChannel, spawn_link};
use crate::ident::Identity;
use crate::shared_sock::{action, bind, err_json, ok_json, read_line, reply, run_subscriber};
use crate::state::HelperState;
use crate::vehicle::VehicleFeed;
use crate::{AsyncAuth, BtError, events};

#[derive(Clone, Default)]
pub struct Broadcaster {
    subs: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>,
    joined: Arc<Notify>,
}

impl Broadcaster {
    pub fn push_json(&self, line: String) {
        self.subs
            .lock()
            .unwrap()
            .retain(|tx| tx.send(line.clone()).is_ok());
    }

    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subs.lock().unwrap().push(tx);
        self.joined.notify_waiters();
        rx
    }

    pub async fn subscribed(&self) {
        loop {
            let joined = self.joined.notified();
            if !self.subs.lock().unwrap().is_empty() {
                return;
            }
            joined.await;
        }
    }
}

#[derive(Clone)]
pub struct Bluez {
    pub bus: zbus::Connection,
    pub adapter: String,
    pub bt_mac: [u8; 6],
}

#[derive(Clone)]
pub struct LiviSockConfig {
    pub path: PathBuf,
    pub identity: Identity,
    pub cp: CpConfig,
    pub disconnect: Option<DropLink>,
    pub targets: Option<PushTargets>,
    pub cp_live: Option<CpFactory>,
}

pub type DropLink = Arc<dyn Fn(String) -> Result<(), BtError> + Send + Sync>;

pub type CpFactory = Arc<dyn Fn() -> CpConfig + Send + Sync>;

pub type PushTargets =
    Arc<dyn Fn(Vec<(String, Option<String>)>) -> Result<(), BtError> + Send + Sync>;

pub async fn serve<A>(
    cfg: LiviSockConfig,
    auth: A,
    bluez: watch::Receiver<Option<Bluez>>,
    bcast: Broadcaster,
    state: Arc<HelperState>,
) -> io::Result<()>
where
    A: AsyncAuth + Clone + Send + 'static,
{
    let listener = bind(&cfg.path, "cp-sock")?;

    loop {
        let (stream, _) = listener.accept().await?;
        let auth = auth.clone();
        let cfg = cfg.clone();
        let bluez = bluez.borrow().clone();
        let bcast = bcast.clone();
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, auth, cfg, bluez, bcast, state).await {
                eprintln!("[cp-sock] connection error: {e}");
            }
        });
    }
}

async fn handle<A>(
    mut stream: UnixStream,
    mut auth: A,
    cfg: LiviSockConfig,
    bluez: Option<Bluez>,
    bcast: Broadcaster,
    state: Arc<HelperState>,
) -> io::Result<()>
where
    A: AsyncAuth + Clone + Send + 'static,
{
    let line = read_line(&mut stream).await?;
    let request = match CpRequest::parse(&line) {
        Ok(request) => request,
        Err(e) => return reply(&mut stream, &err_json(&e)).await,
    };

    let json = match request {
        CpRequest::Subscribe => return run_subscriber(stream, bcast, "cp-sock").await,
        CpRequest::Tunnel { cid, bt_mac } => {
            let shown = bt_mac.as_deref().unwrap_or("unknown");
            if state.carkit_blocks(bt_mac.as_deref().unwrap_or("")) {
                println!(
                    "[cp-sock] tunnel refused, iAP2 already runs over USB carkit (cid={cid}, btMac={shown})"
                );
                return Ok(());
            }
            println!("[cp-sock] tunnel up (cid={cid}, btMac={shown})");
            let cfg = match &bluez {
                Some(bluez) => LiviSockConfig {
                    identity: Identity {
                        bt_mac: bluez.bt_mac,
                        ..cfg.identity
                    },
                    ..cfg
                },
                None => cfg,
            };
            run_tunnel(stream, auth, cfg, bcast, cid, state.vehicle_feed());
            return Ok(());
        }
        CpRequest::Certificate => {
            match (auth.read_certificate().await, auth.protocol_major().await) {
                (Ok(cert), Ok(major)) => format!(
                    "{{\"ok\":true,\"data\":\"{}\",\"protocolMajor\":{}}}",
                    STANDARD.encode(&cert),
                    major
                ),
                (Err(e), _) | (_, Err(e)) => err_json(&format!("MFi: {e}")),
            }
        }
        CpRequest::Sign(digest) => match auth.sign(digest).await {
            Ok(sig) => format!("{{\"ok\":true,\"data\":\"{}\"}}", STANDARD.encode(&sig)),
            Err(e) => err_json(&e.to_string()),
        },
        CpRequest::Disconnect(mac) => match bluez.as_ref() {
            Some(bluez) => action(device_disconnect(&bluez.bus, &bluez.adapter, &mac).await),
            None => match cfg.disconnect.clone() {
                Some(drop) => match tokio::task::spawn_blocking(move || drop(mac)).await {
                    Ok(done) => action(done),
                    Err(e) => err_json(&e.to_string()),
                },
                None => err_json("nothing here can drop a bluetooth link"),
            },
        },
        CpRequest::ReconnectTargets(targets) => {
            let before = state.reconnect_targets();
            state.set_reconnect_targets(targets);
            let after = state.reconnect_targets();
            if before != after
                && let Some(push) = cfg.targets.clone()
            {
                let macs: Vec<&str> = after.iter().map(|(mac, _)| mac.as_str()).collect();
                println!("[helperd] the dongle pages {macs:?}");
                if let Ok(Err(e)) = tokio::task::spawn_blocking(move || push(after)).await {
                    eprintln!("[helperd] the dongle refused the paging list: {e}");
                }
            }
            ok_json()
        }
        CpRequest::Location(nmea) => {
            state.vehicle().push_location(nmea);
            ok_json()
        }
        CpRequest::VehicleStatus(status) => {
            state.vehicle().push_status(&status);
            ok_json()
        }
        CpRequest::Seek { ms, bt_mac } => {
            state.vehicle().push_seek(ms, bt_mac);
            ok_json()
        }
        CpRequest::DropIap2 => {
            let n = state.restart_wired();
            println!("[cp-sock] drop-iap2: {n} wired session(s) end for a fresh start");
            ok_json()
        }
        CpRequest::StartWired(udid) => {
            if state.start_wired_again(&udid) {
                println!("[cp-sock] start-wired: {udid} offers CarPlay again");
                ok_json()
            } else {
                err_json(&format!("no wired session for {udid:?}"))
            }
        }
        CpRequest::ResetWired(udid) => {
            if state.reset_wired(&udid) {
                println!("[cp-sock] reset-wired: {udid} gets a USB reset");
                ok_json()
            } else {
                err_json(&format!("no wired session for {udid:?}"))
            }
        }
    };
    reply(&mut stream, &json).await
}

fn run_tunnel<A>(
    stream: UnixStream,
    auth: A,
    cfg: LiviSockConfig,
    bcast: Broadcaster,
    cid: String,
    vehicle: VehicleFeed,
) where
    A: AsyncAuth + Clone + Send + 'static,
{
    let Ok(std_stream) = stream.into_std() else {
        return;
    };
    let fd: OwnedFd = std_stream.into();
    let link_cfg = LinkConfig {
        max_outgoing: 4,
        control_version: 2,
        zero_ack: true,
        ..LinkConfig::default()
    };
    let (channel, art_rx) = spawn_link(fd, link_cfg, true);
    let (tx, rx) = mpsc::channel(64);
    let (base, live, identity) = (cfg.cp, cfg.cp_live, cfg.identity);
    tokio::spawn(async move {
        let cp = match live {
            Some(resolve) => tokio::task::spawn_blocking(move || resolve())
                .await
                .unwrap_or(base),
            None => base,
        };
        run_accessory(channel, auth, identity, cp, tx, vehicle).await;
    });
    let ident: SharedTag = Arc::new(Mutex::new(events::EventTag {
        cid: (!cid.is_empty()).then_some(cid),
        ..Default::default()
    }));
    tokio::spawn(pump_events_for(
        rx,
        bcast.clone(),
        "tunnel",
        None,
        ident.clone(),
    ));
    tokio::spawn(pump_artwork(art_rx, bcast, ident));
}

pub fn run_bluetooth<A>(
    link: impl FnOnce(LinkConfig) -> (LinkChannel, ArtworkRx),
    auth: A,
    identity: Identity,
    cp: CpConfig,
    bcast: Broadcaster,
    state: Arc<HelperState>,
    peer: String,
) where
    A: AsyncAuth + Send + 'static,
{
    if cp.on_cable.as_ref().is_some_and(|cabled| (cabled.0)(&peer)) {
        println!("[bt] {peer} is on the cable, its Bluetooth link goes");
        return;
    }
    println!("[bt] phone connected mac={peer}");
    let (channel, art_rx) = link(LinkConfig {
        max_outgoing: 4,
        control_version: 2,
        ..LinkConfig::default()
    });
    let (tx, rx) = mpsc::channel(64);
    let accessory = run_accessory(channel, auth, identity, cp, tx, state.vehicle_feed());
    tokio::spawn(async move {
        state.link_up(&peer);
        accessory.await;
        state.link_down(&peer);
    });
    let ident: SharedTag = Default::default();
    tokio::spawn(pump_events_for(
        rx,
        bcast.clone(),
        "bt",
        None,
        ident.clone(),
    ));
    tokio::spawn(pump_artwork(art_rx, bcast, ident));
}

pub async fn pump_artwork(
    mut art_rx: crate::driver::ArtworkRx,
    bcast: Broadcaster,
    ident: SharedTag,
) {
    while let Some(data) = art_rx.recv().await {
        if data.is_empty() {
            continue;
        }
        let art = HelperKind::NowPlaying(NowPlayingPatch {
            artwork: Some(STANDARD.encode(&data)),
            ..Default::default()
        });
        bcast.push_json(ident.lock().unwrap().tagged(art));
    }
}

pub type SharedTag = Arc<Mutex<events::EventTag>>;

pub async fn pump_events_for(
    mut rx: mpsc::Receiver<BringupEvent>,
    bcast: Broadcaster,
    tag: &'static str,
    usb_udid: Option<String>,
    ident: SharedTag,
) {
    let mut time_synced = false;
    while let Some(event) = rx.recv().await {
        match event {
            BringupEvent::Incoming { frame, .. } => {
                if !time_synced && let Some(secs) = events::device_time(&frame) {
                    time_synced = true;
                    tokio::task::spawn_blocking(move || crate::clock::step_to(secs));
                }
                let tagged = {
                    let mut t = ident.lock().unwrap();
                    t.learn(&frame);
                    (
                        events::device_event(&frame, usb_udid.as_deref()).map(|k| t.tagged(k)),
                        events::phone_event(&frame)
                            .or_else(|| t.navigation_event(&frame))
                            .map(|k| t.tagged(k)),
                    )
                };
                if let Some(json) = tagged.0 {
                    bcast.push_json(json);
                }
                if let Some(json) = tagged.1 {
                    bcast.push_json(json);
                }
            }
            BringupEvent::Failed(e) => eprintln!("[cp-sock] {tag} bring-up failed: {e}"),
            BringupEvent::Identified => println!("[cp] {tag}: identification accepted"),
            BringupEvent::Authenticated => println!("[cp] {tag}: MFi auth succeeded"),
            BringupEvent::CarPlayStartSent { ip } => {
                println!("[cp] {tag}: CarPlayStartSession sent");
                if let Some(udid) = usb_udid.as_deref() {
                    let start = events::wired_start_event(udid, &ip);
                    bcast.push_json(ident.lock().unwrap().tagged(start));
                }
            }
            _ => {}
        }
    }
}

pub async fn device_disconnect(
    bus: &zbus::Connection,
    adapter: &str,
    mac: &str,
) -> Result<(), BtError> {
    let path = format!(
        "/org/bluez/{}/dev_{}",
        adapter,
        mac.replace(':', "_").to_uppercase()
    );
    bus.call_method(
        Some("org.bluez"),
        path.as_str(),
        Some("org.bluez.Device1"),
        "Disconnect",
        &(),
    )
    .await?;
    Ok(())
}
