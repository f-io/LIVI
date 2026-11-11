// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use livi_helper_stack::bringup::{AskOnAir, CpConfig, OnCable};
use livi_helper_stack::driver::spawn_link;
use livi_helper_stack::hfp::Hfp;
use livi_helper_stack::ident::{Identity, Transport};
use livi_helper_stack::livi_sock::{Broadcaster, DropLink, run_bluetooth};
use livi_helper_stack::mfi_async::SharedCoprocessor;
use livi_helper_stack::reconnect::{self, Paging};
use livi_helper_stack::shared_sock::Bluetooth;
use livi_helper_stack::state::HelperState;
use livi_mdns::responder::Responder;
use livi_wifi::{Channel, OnAir, Security};
use tokio::sync::Notify;
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
const AP_RETRY_FIRST: Duration = Duration::from_secs(5);
const AP_RETRY_LONGEST: Duration = Duration::from_secs(80);

pub struct Wireless {
    pub auth: SharedCoprocessor,
    pub identity: Identity,
    pub cp: CpConfig,
    pub bcast: Broadcaster,
    pub state: Arc<HelperState>,
    pub airplay: crate::wired::Airplay,
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
static OFF_AIR: Notify = Notify::const_new();

fn on_air() -> Option<OnAir> {
    ON_AIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn off_air() {
    *ON_AIR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    OFF_AIR.notify_one();
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

pub async fn listen(phones: Phones, hotspot: Hotspot, page: Page, ctx: Wireless) {
    let mdns = on_air_with(&hotspot, &ctx.cp, &ctx.airplay, ctx.carplay).await;
    tokio::spawn(keep_on_air(
        hotspot,
        ctx.cp.clone(),
        ctx.airplay.clone(),
        ctx.carplay,
        mdns,
    ));
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
    tokio::spawn(reconnect::run(
        Pager { page, wakes, hfp },
        ctx.state.clone(),
    ));
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
            ("connect", Some(uuid)) => page(&mac, uuid).map_err(Into::into),
            ("disconnect", _) => drop(mac),
            _ => Err(format!("{verb} is not available on Android").into()),
        }
    })
}

fn carplay_phone(ctx: &Wireless, fd: OwnedFd, peer: String) {
    let drop = ctx.drop.clone();
    let cp = CpConfig {
        on_cable: Some(OnCable::dropping(ctx.state.clone(), move |mac| {
            if let Err(e) = drop(mac.to_string()) {
                eprintln!("[wireless] {e}");
            }
        })),
        ..session_cp(&ctx.cp)
    };
    run_bluetooth(
        |cfg| spawn_link(fd, cfg, false),
        ctx.auth.clone(),
        ctx.identity.clone(),
        cp,
        ctx.bcast.clone(),
        ctx.state.clone(),
        peer,
    );
}

fn android_auto_phone(aa: &AndroidAuto, state: Arc<HelperState>, fd: OwnedFd, mac: String) {
    println!("[aa] phone connected mac={mac}");
    let Some(told) = offer() else {
        println!("[aa] {mac}: no access point to offer");
        return;
    };
    let (events, wired) = (aa.events.clone(), aa.wired.clone());
    tokio::spawn(async move {
        match crate::aa::stream(fd) {
            Ok(sock) => crate::aa::bootstrap(sock, &mac, told, &events, &wired, &state).await,
            Err(e) => eprintln!("[aa] {mac}: {e}"),
        }
    });
}

fn offer() -> Option<Told> {
    let granted = GRANTED.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let ap_ip = livi_helper_stack::net::ipv4_of(&granted.iface)?.to_string();
    let bssid = livi_helper_stack::net::wlan_mac(&granted.iface).unwrap_or_default();
    let told = AaConfig {
        channel: Channel::of_freq(granted.frequency)?,
        ssid: granted.ssid,
        passphrase: granted.passphrase,
        ap_ip,
        port: livi_aa::consts::TCP_PORT,
    };
    Some((told, Security::Wpa2, bssid))
}

struct Pager {
    page: Page,
    wakes: Vec<&'static str>,
    hfp: Option<Hfp>,
}

impl Pager {
    fn wake(&self, wake: Option<&str>) -> Option<&'static str> {
        self.wakes.iter().find(|w| wake == Some(**w)).copied()
    }
}

impl Paging for Pager {
    fn ready(&self) -> bool {
        on_air().is_some()
    }

    fn wanted(&self, wake: Option<&str>) -> bool {
        self.wake(wake).is_some()
    }

    fn served(&self, mac: &str, wake: Option<&str>) -> bool {
        wake == Some(HFP_AG_UUID) && self.hfp.as_ref().is_some_and(|h| h.linked(mac))
    }

    async fn page(&self, mac: &str, wake: Option<&str>) -> Option<bool> {
        let wake = self.wake(wake)?;
        println!("[wireless] paging {mac}");
        let (page, peer) = (self.page.clone(), mac.to_string());
        match tokio::task::spawn_blocking(move || page(&peer, wake)).await {
            Ok(Ok(())) => {
                println!("[wireless] {mac} answered the page");
                Some(true)
            }
            Ok(Err(e)) => {
                println!("[wireless] {mac} page failed: {e}");
                Some(false)
            }
            Err(e) => {
                eprintln!("[wireless] {e}");
                Some(false)
            }
        }
    }
}

async fn keep_on_air(
    hotspot: Hotspot,
    base: CpConfig,
    airplay: crate::wired::Airplay,
    carplay: bool,
    mut mdns: Vec<Responder>,
) {
    let mut retry = AP_RETRY_FIRST;
    loop {
        if on_air().is_some() {
            retry = AP_RETRY_FIRST;
            OFF_AIR.notified().await;
            continue;
        }
        mdns.clear();
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(AP_RETRY_LONGEST);
        mdns = on_air_with(&hotspot, &base, &airplay, carplay).await;
    }
}

async fn on_air_with(
    hotspot: &Hotspot,
    base: &CpConfig,
    airplay: &crate::wired::Airplay,
    carplay: bool,
) -> Vec<Responder> {
    let request = ApRequest {
        ssid: base.ssid.clone(),
        passphrase: base.passphrase.clone(),
        frequency: base.channel.freq_mhz(),
    };
    let hotspot = hotspot.clone();
    let granted = match tokio::task::spawn_blocking(move || hotspot(&request)).await {
        Ok(Ok(granted)) => granted,
        Ok(Err(e)) => {
            eprintln!("[wireless] no access point: {e}");
            return Vec::new();
        }
        Err(e) => {
            eprintln!("[wireless] {e}");
            return Vec::new();
        }
    };
    *ON_AIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(OnAir {
        ssid: granted.ssid.clone(),
        channel: Channel::of_freq(granted.frequency).unwrap_or(base.channel),
        security: Security::Wpa2,
    });
    let iface = granted.iface.clone();
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(granted);
    if !carplay && airplay.mirror.is_none() {
        return Vec::new();
    }
    let Some(address) =
        livi_helper_stack::net::wlan_link_local(&iface).and_then(|a| a.parse().ok())
    else {
        eprintln!("[wireless] {iface} has no link-local address, AirPlay is not announced");
        return Vec::new();
    };
    crate::wired::announce(&iface, address, &session_cp(base), airplay, carplay)
}
