// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iap2_link::LinkConfig;
use livi_mdns::responder::Responder;
use livi_runtime::bringup::{AskOnAir, CpConfig, OnCable, run_accessory};
use livi_runtime::driver::spawn_link;
use livi_runtime::hfp::Hfp;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{Broadcaster, DropLink, SharedTag, pump_artwork, pump_events_for};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::shared_sock::Bluetooth;
use livi_runtime::state::HelperState;
use livi_wifi::{Channel, OnAir, Security};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::aa::{AaConfig, Told, WiredPhones};

pub enum Service {
    Iap,
    HandsFree,
    AndroidAuto,
}

pub struct Phone {
    pub fd: OwnedFd,
    pub peer: String,
    pub service: Service,
}

pub type Phones =
    Arc<dyn Fn(&str, bool, bool) -> Result<UnboundedReceiver<Phone>, String> + Send + Sync>;

pub struct ApRequest {
    pub ssid: String,
    pub passphrase: String,
    pub frequency: u32,
}

#[derive(Clone)]
pub struct ApGranted {
    pub iface: String,
    pub ssid: String,
    pub passphrase: String,
    pub frequency: u32,
}

pub type Hotspot = Arc<dyn Fn(&ApRequest) -> Result<ApGranted, String> + Send + Sync>;

pub type Page = Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;

const IAP_PHONE_UUID: &str = "00000000-deca-fade-deca-deafdecacafe";
const HFP_AG_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";
const FAST_INTERVAL: Duration = Duration::from_secs(1);
const FAST_ATTEMPTS: u32 = 15;
const SLOW_INTERVAL: Duration = Duration::from_secs(30);

pub struct Wireless {
    pub auth: SharedCoprocessor,
    pub identity: Identity,
    pub cp: CpConfig,
    pub bcast: Broadcaster,
    pub state: Arc<HelperState>,
    pub airplay_txt: Vec<String>,
    pub drop: DropLink,
    pub carplay: bool,
    pub android_auto: Option<AndroidAuto>,
}

pub struct AndroidAuto {
    pub hfp: Hfp,
    pub events: Broadcaster,
    pub wired: WiredPhones,
}

static ON_AIR: Mutex<Option<OnAir>> = Mutex::new(None);
static GRANTED: Mutex<Option<ApGranted>> = Mutex::new(None);

fn on_air() -> Option<OnAir> {
    ON_AIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn off_air() {
    *ON_AIR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn session_cp(base: &CpConfig) -> CpConfig {
    let mut cp = CpConfig {
        transport: Transport::Wireless,
        ..base.clone()
    };
    if let Some(granted) = GRANTED.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        cp.ssid = granted.ssid;
        cp.passphrase = granted.passphrase;
        cp.wifi_iface = granted.iface;
        cp.channel = Channel::of_freq(granted.frequency).unwrap_or(cp.channel);
        cp.ap_on_air = Some(on_air as AskOnAir);
    }
    cp
}

pub async fn listen(phones: Phones, hotspot: Hotspot, page: Page, mut ctx: Wireless) {
    let _mdns = on_air_with(hotspot, &mut ctx).await;
    let name = ctx.identity.name.clone();
    let (carplay, android_auto) = (ctx.carplay, ctx.android_auto.is_some());
    let mut phones =
        match tokio::task::spawn_blocking(move || phones(&name, carplay, android_auto)).await {
            Ok(Ok(phones)) => phones,
            Ok(Err(e)) => {
                eprintln!("[wireless] {e}, wireless projection is off");
                return;
            }
            Err(e) => {
                eprintln!("[wireless] {e}");
                return;
            }
        };
    if carplay {
        println!("[wireless] CarPlay waits for phones on Bluetooth");
    }
    if android_auto {
        println!("[wireless] Android Auto waits for phones on Bluetooth");
    }
    let mut wakes = Vec::new();
    if carplay {
        wakes.push(IAP_PHONE_UUID);
    }
    if android_auto {
        wakes.push(HFP_AG_UUID);
    }
    let hfp = ctx.android_auto.as_ref().map(|aa| aa.hfp.clone());
    tokio::spawn(page_known(ctx.state.clone(), page, wakes, hfp));
    while let Some(phone) = phones.recv().await {
        match (phone.service, &ctx.android_auto) {
            (Service::Iap, _) if ctx.carplay => carplay_phone(&ctx, phone.fd, phone.peer),
            (Service::HandsFree, Some(aa)) => aa.hfp.accept(phone.fd, phone.peer),
            (Service::AndroidAuto, Some(aa)) => {
                android_auto_phone(aa, ctx.state.clone(), phone.fd, phone.peer)
            }
            _ => println!("[wireless] {} opened a service that is off", phone.peer),
        }
    }
}

pub fn bluetooth(page: Page, drop: DropLink, hfp: Option<Hfp>) -> Bluetooth {
    Arc::new(move |verb, arg| {
        let (mac, uuid) = match arg.split_once(' ') {
            Some((mac, uuid)) => (mac.to_uppercase(), Some(uuid.trim())),
            None => (arg.to_uppercase(), None),
        };
        match (verb, uuid) {
            ("connect", Some(HFP_AG_UUID)) if hfp.as_ref().is_some_and(|h| h.linked(&mac)) => {
                Ok(())
            }
            ("connect", Some(uuid)) => page(&mac, uuid),
            ("disconnect", _) => drop(mac),
            _ => Err(format!("{verb} is not available on Android")),
        }
    })
}

fn carplay_phone(ctx: &Wireless, fd: OwnedFd, peer: String) {
    if ctx.state.carkit_claims(&peer) {
        println!("[wireless] {peer} is on the cable, its Bluetooth link goes");
        return;
    }
    println!("[wireless] phone connected mac={peer}");
    let drop = ctx.drop.clone();
    let links = ctx.state.clone();
    let cp = CpConfig {
        on_cable: Some(OnCable(Arc::new(move |mac: &str| {
            let cabled = links.carkit_claims(mac);
            if cabled && let Err(e) = drop(mac.to_string()) {
                eprintln!("[wireless] {e}");
            }
            cabled
        }))),
        ..ctx.cp.clone()
    };
    let cfg = LinkConfig {
        max_outgoing: 4,
        control_version: 2,
        ..LinkConfig::default()
    };
    let (channel, art_rx) = spawn_link(fd, cfg, false);
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let accessory = run_accessory(
        channel,
        ctx.auth.clone(),
        ctx.identity.clone(),
        cp,
        tx,
        ctx.state.vehicle_feed(),
    );
    let (links, mac) = (ctx.state.clone(), peer);
    tokio::spawn(async move {
        links.link_up(&mac);
        accessory.await;
        links.link_down(&mac);
    });
    let ident: SharedTag = Default::default();
    tokio::spawn(pump_events_for(
        rx,
        ctx.bcast.clone(),
        "bt",
        None,
        ident.clone(),
    ));
    tokio::spawn(pump_artwork(art_rx, ctx.bcast.clone(), ident));
}

fn android_auto_phone(aa: &AndroidAuto, state: Arc<HelperState>, fd: OwnedFd, mac: String) {
    println!("[aa] phone connected mac={mac}");
    let Some(told) = offer() else {
        println!("[aa] {mac}: no access point to offer");
        return;
    };
    let (events, wired) = (aa.events.clone(), aa.wired.clone());
    tokio::spawn(async move {
        let sock = match stream(fd) {
            Ok(sock) => sock,
            Err(e) => {
                eprintln!("[aa] {mac}: {e}");
                return;
            }
        };
        state.link_up(&mac);
        let ended = crate::aa::handshake(sock, &mac, told, &events, &wired).await;
        state.link_down(&mac);
        if let Err(e) = ended {
            println!("[aa] {mac}: bootstrap ended: {e}");
        }
    });
}

fn stream(fd: OwnedFd) -> std::io::Result<tokio::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(stream)
}

fn offer() -> Option<Told> {
    let granted = GRANTED.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let ap_ip = livi_runtime::net::ipv4_of(&granted.iface)?.to_string();
    let bssid = livi_runtime::net::wlan_mac(&granted.iface).unwrap_or_default();
    let told = AaConfig {
        channel: Channel::of_freq(granted.frequency)?,
        ssid: granted.ssid,
        passphrase: granted.passphrase,
        ap_ip,
        port: livi_aa::consts::TCP_PORT,
    };
    Some((told, Security::Wpa2, bssid))
}

async fn page_known(
    state: Arc<HelperState>,
    page: Page,
    wakes: Vec<&'static str>,
    hfp: Option<Hfp>,
) {
    let mut turn: usize = 0;
    let mut attempts: HashMap<String, u32> = HashMap::new();
    let mut next_try: HashMap<String, Instant> = HashMap::new();
    loop {
        tokio::time::sleep(FAST_INTERVAL).await;
        let targets: Vec<(String, &'static str)> = state
            .reconnect_targets()
            .into_iter()
            .filter_map(|(mac, uuid)| {
                let wake = wakes.iter().find(|w| uuid.as_deref() == Some(**w))?;
                Some((mac, *wake))
            })
            .collect();
        attempts.retain(|mac, _| targets.iter().any(|(m, _)| m == mac));
        next_try.retain(|mac, _| targets.iter().any(|(m, _)| m == mac));
        if targets.is_empty() {
            continue;
        }
        let (mac, wake) = targets[turn % targets.len()].clone();
        turn = turn.wrapping_add(1);
        let hands_free = wake == HFP_AG_UUID && hfp.as_ref().is_some_and(|h| h.linked(&mac));
        if state.link_active(&mac) || hands_free {
            attempts.remove(&mac);
            next_try.remove(&mac);
            continue;
        }
        if on_air().is_none() || next_try.get(&mac).is_some_and(|t| Instant::now() < *t) {
            continue;
        }
        println!("[wireless] paging {mac}");
        let (page, peer) = (page.clone(), mac.clone());
        match tokio::task::spawn_blocking(move || page(&peer, wake)).await {
            Ok(Ok(())) => {
                println!("[wireless] {mac} answered the page");
                attempts.remove(&mac);
                next_try.remove(&mac);
                continue;
            }
            Ok(Err(e)) => println!("[wireless] {mac} page failed: {e}"),
            Err(e) => eprintln!("[wireless] {e}"),
        }
        let n = attempts.entry(mac.clone()).or_insert(0);
        *n += 1;
        if *n >= FAST_ATTEMPTS {
            next_try.insert(mac, Instant::now() + SLOW_INTERVAL);
        }
    }
}

async fn on_air_with(hotspot: Hotspot, ctx: &mut Wireless) -> Option<Responder> {
    let request = ApRequest {
        ssid: ctx.cp.ssid.clone(),
        passphrase: ctx.cp.passphrase.clone(),
        frequency: ctx.cp.channel.freq_mhz(),
    };
    let granted = match tokio::task::spawn_blocking(move || hotspot(&request)).await {
        Ok(Ok(granted)) => granted,
        Ok(Err(e)) => {
            eprintln!("[wireless] no access point: {e}");
            return None;
        }
        Err(e) => {
            eprintln!("[wireless] {e}");
            return None;
        }
    };
    *ON_AIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(OnAir {
        ssid: granted.ssid.clone(),
        channel: Channel::of_freq(granted.frequency).unwrap_or(ctx.cp.channel),
        security: Security::Wpa2,
    });
    let iface = granted.iface.clone();
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(granted);
    ctx.cp = session_cp(&ctx.cp);
    if !ctx.carplay {
        return None;
    }
    let Some(address) = livi_runtime::net::wlan_link_local(&iface).and_then(|a| a.parse().ok())
    else {
        eprintln!("[wireless] {iface} has no link-local address, CarPlay is not announced");
        return None;
    };
    crate::wired::announce(&iface, address, &ctx.cp, &ctx.airplay_txt)
}
