// The dongle's Bluetooth controller, driven from here over its HCI tunnel by our own host, the way
// BlueZ drives it on Linux.

use std::collections::{HashMap, HashSet};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use livi_bt_host::sdp::{self, Service};
use livi_bt_host::{Call, Config, Inbox, Incoming, Notice, Stack, hci};
use livi_runtime::bringup::OnCable;
use livi_runtime::hfp::Hfp;
use livi_runtime::livi_sock::Broadcaster;
use livi_runtime::sco::ScoSink;
use livi_runtime::state::HelperState;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use crate::link::LinkPresence;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY: Duration = Duration::from_secs(5);
const PING_TIMEOUT: Duration = Duration::from_secs(1);
const FAST_INTERVAL: Duration = Duration::from_secs(1);
const FAST_ATTEMPTS: u32 = 15;
const SLOW_INTERVAL: Duration = Duration::from_secs(30);
const STALE: Duration = Duration::from_secs(10);

/// A phone on a channel a session is built on.
pub struct Arrival {
    pub peer: String,
    /// The controller's address, most significant byte first: the one the phone talks to.
    pub local: [u8; 6],
    pub stream: tokio::net::UnixStream,
}

/// Where each profile's phones go.
#[derive(Clone)]
pub struct Profiles {
    pub carplay: Option<UnboundedSender<Arrival>>,
    pub android_auto: Option<UnboundedSender<Arrival>>,
    pub hfp: Hfp,
    pub events: Broadcaster,
    pub sco_sink: ScoSink,
}

#[derive(Clone, Default)]
pub struct Dongle(Arc<Mutex<Option<Stack>>>);

impl Dongle {
    fn stack(&self) -> Option<Stack> {
        self.0.lock().unwrap().clone()
    }

    pub fn disconnect(&self, mac: &str) -> Result<(), String> {
        let addr = hci::parse_addr(mac).ok_or_else(|| format!("{mac} is no Bluetooth address"))?;
        let stack = self.stack().ok_or("the dongle's Bluetooth is not up")?;
        if stack.disconnect(&addr) { Ok(()) } else { Err(format!("{mac} is not connected")) }
    }

    /// The dongle went, its links with it.
    pub fn end(&self) {
        if let Some(stack) = self.0.lock().unwrap().take() {
            stack.end();
        }
    }

    /// CarPlay over the cable keeps the phone's Bluetooth to the accessory disconnected.
    pub fn on_cable(&self, state: Arc<HelperState>) -> OnCable {
        let dongle = self.clone();
        OnCable(Arc::new(move |mac: &str| {
            let cabled = state.carkit_claims(mac);
            if cabled && let Err(e) = dongle.disconnect(mac) {
                eprintln!("[bt] {mac} stays on the dongle's bluetooth: {e}");
            }
            cabled
        }))
    }
}

fn keys_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/LIVI/bluetooth-keys")
}

fn open(name: String) -> Result<(Stack, Inbox), String> {
    let at = (livi_link_host::link::LINK_NAME, livi_net::port::HCI);
    let tunnel = livi_net::connect(at, CONNECT_TIMEOUT)
        .map_err(|e| format!("the dongle's controller: {e}"))?;
    let _ = tunnel.set_nodelay(true);
    Stack::start(tunnel, Config { name, keys: keys_path() })
}

pub async fn run(
    dongle: Dongle,
    link: Arc<LinkPresence>,
    profiles: Profiles,
    state: Arc<HelperState>,
    offered: Vec<Service>,
    name: String,
) {
    let mic = match livi_runtime::sco::mic_socket() {
        Ok(mic) => Arc::new(mic),
        Err(e) => {
            eprintln!("[sco] mic socket failed: {e}");
            return;
        }
    };
    loop {
        link.wait_until(true).await;
        let named = name.clone();
        let (stack, inbox) = match tokio::task::spawn_blocking(move || open(named)).await {
            Ok(Ok(up)) => up,
            Ok(Err(e)) => {
                eprintln!("[bt] {e}");
                tokio::time::sleep(RETRY).await;
                continue;
            }
            Err(_) => return,
        };
        for service in &offered {
            if let Err(e) = stack.offer(*service, true) {
                eprintln!("[bt] {e}");
            }
        }
        *dongle.0.lock().unwrap() = Some(stack.clone());
        let Inbox { incoming, calls, notices } = inbox;
        let rt = Handle::current();
        let (s, p, r) = (stack.clone(), profiles.clone(), rt.clone());
        std::thread::spawn(move || {
            for arrived in incoming {
                take(&r, &s, &p, arrived);
            }
        });
        let (p, m) = (profiles.clone(), mic.clone());
        std::thread::spawn(move || {
            for call in calls {
                answer(call, &p, &m);
            }
        });
        tokio::spawn(page(stack.clone(), state.clone(), profiles.clone(), rt));
        let _ =
            tokio::task::spawn_blocking(move || notices.iter().find(|n| *n == Notice::Ended)).await;
        dongle.0.lock().unwrap().take();
        tokio::time::sleep(RETRY).await;
    }
}

fn take(rt: &Handle, stack: &Stack, profiles: &Profiles, arrived: Incoming) {
    let mac = hci::text(&arrived.peer);
    match Service::of_channel(arrived.channel) {
        Some(Service::HandsFree) => {
            let _inside = rt.enter();
            profiles.hfp.accept(OwnedFd::from(arrived.stream), mac);
        }
        Some(Service::CarPlay) => hand(rt, stack, profiles.carplay.as_ref(), mac, arrived.stream),
        Some(Service::AndroidAuto) => {
            hand(rt, stack, profiles.android_auto.as_ref(), mac, arrived.stream);
        }
        None => {}
    }
}

fn hand(
    rt: &Handle,
    stack: &Stack,
    to: Option<&UnboundedSender<Arrival>>,
    peer: String,
    stream: std::os::unix::net::UnixStream,
) {
    let Some(to) = to else { return };
    let _inside = rt.enter();
    let Ok(stream) =
        stream.set_nonblocking(true).and_then(|()| tokio::net::UnixStream::from_std(stream))
    else {
        return;
    };
    let mut local = stack.local();
    local.reverse();
    let _ = to.send(Arrival { peer, local, stream });
}

fn answer(call: Call, profiles: &Profiles, mic: &Arc<UnixListener>) {
    let link = livi_hfp::sco::Link {
        fd: OwnedFd::from(call.socket),
        // Our own host sends each packet whole, there is no kernel limit in between.
        mtu: livi_hfp::sco::MAX_PACKET,
        peer: call.peer,
    };
    let (events, sink, mic) = (profiles.events.clone(), profiles.sco_sink.clone(), mic.clone());
    std::thread::spawn(move || livi_runtime::sco::call(&events, &link, &mic, &sink));
}

fn parse_uuid(text: &str) -> Option<[u8; 16]> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// livi-core pages an Android Auto phone through its hands-free gateway.
fn wakes_hands_free(wake: Option<&str>) -> bool {
    wake.and_then(parse_uuid) == Some(sdp::full_uuid(sdp::HANDS_FREE_GATEWAY))
}

/// The SDP element of the UUID a target is woken by, iAP when it names none.
fn service_element(wake: Option<&str>) -> Vec<u8> {
    let Some(uuid) = wake.and_then(parse_uuid) else {
        return sdp::uuid_of_128(&sdp::IAP_CLIENT_UUID);
    };
    let base = sdp::full_uuid(0);
    if uuid[..2] == [0, 0] && uuid[4..] == base[4..] {
        sdp::uuid_of(u16::from_be_bytes([uuid[2], uuid[3]]))
    } else {
        sdp::uuid_of_128(&uuid)
    }
}

/// Opens the target's profile in the background. Its stream goes to the profile whenever it
/// arrives, the rotation does not wait for it.
fn poke(
    stack: &Stack,
    profiles: &Profiles,
    rt: &Handle,
    busy: &Arc<Mutex<HashSet<String>>>,
    mac: &str,
    wake: Option<&str>,
) -> Option<tokio::task::JoinHandle<bool>> {
    let addr = hci::parse_addr(mac)?;
    if !busy.lock().unwrap().insert(mac.to_string()) {
        return None;
    }
    let (stack, profiles, rt, busy) = (stack.clone(), profiles.clone(), rt.clone(), busy.clone());
    let (mac, gateway, service) = (mac.to_string(), wakes_hands_free(wake), service_element(wake));
    Some(rt.clone().spawn_blocking(move || {
        let opened = stack.connect(addr, &service);
        busy.lock().unwrap().remove(&mac);
        match opened {
            Ok(stream) if gateway => {
                println!("[bt] {mac} answered");
                let _inside = rt.enter();
                profiles.hfp.accept(OwnedFd::from(stream), mac);
                true
            }
            Ok(stream) => {
                println!("[bt] {mac} answered");
                hand(&rt, &stack, profiles.carplay.as_ref(), mac, stream);
                true
            }
            Err(e) => {
                println!("[bt] {mac} page failed: {e}");
                false
            }
        }
    }))
}

/// Round-robins the phones LIVI wants back, as the reconnect loop does over BlueZ: each page is
/// abandoned after a second and runs on in the background, so an absent phone does not hold up
/// the one that is there.
async fn page(stack: Stack, state: Arc<HelperState>, profiles: Profiles, rt: Handle) {
    let mut turn: usize = 0;
    let mut attempts: HashMap<String, u32> = HashMap::new();
    let mut next_try: HashMap<String, Instant> = HashMap::new();
    let mut stale_since: HashMap<String, Instant> = HashMap::new();
    let busy: Arc<Mutex<HashSet<String>>> = Arc::default();

    while stack.alive() {
        tokio::time::sleep(FAST_INTERVAL).await;
        let targets = state.reconnect_targets();
        let listed = |mac: &String| targets.iter().any(|(m, _)| m == mac);
        attempts.retain(|mac, _| listed(mac));
        next_try.retain(|mac, _| listed(mac));
        stale_since.retain(|mac, _| listed(mac));
        if targets.is_empty() {
            continue;
        }
        let (mac, wake) = targets[turn % targets.len()].clone();
        turn = turn.wrapping_add(1);
        let Some(addr) = hci::parse_addr(&mac) else { continue };

        if stack.connected(&addr) {
            attempts.remove(&mac);
            next_try.remove(&mac);
            // A link in progress, for instance still waiting for the access point, is not stale.
            if state.link_active(&mac) {
                stale_since.remove(&mac);
                continue;
            }
            let since = *stale_since.entry(mac.clone()).or_insert_with(Instant::now);
            if since.elapsed() >= STALE {
                stale_since.remove(&mac);
                println!("[bt] {mac} connected but no session, disconnecting");
                stack.disconnect(&addr);
            } else if !stack.in_use(&addr) {
                poke(&stack, &profiles, &rt, &busy, &mac, wake.as_deref());
            }
            continue;
        }
        stale_since.remove(&mac);

        if next_try.get(&mac).is_some_and(|t| Instant::now() < *t) {
            continue;
        }
        // A page holds the radio for seconds, a call's audio would not get through.
        if profiles.hfp.in_call() {
            continue;
        }
        let Some(paging) = poke(&stack, &profiles, &rt, &busy, &mac, wake.as_deref()) else {
            continue;
        };
        println!("[bt] paging {mac}");
        if let Ok(Ok(true)) = tokio::time::timeout(PING_TIMEOUT, paging).await {
            attempts.remove(&mac);
            next_try.remove(&mac);
            continue;
        }
        let n = attempts.entry(mac.clone()).or_insert(0);
        *n += 1;
        if *n >= FAST_ATTEMPTS {
            next_try.insert(mac, Instant::now() + SLOW_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_is_woken_by_the_service_it_names() {
        let gateway = "0000111f-0000-1000-8000-00805f9b34fb";
        assert!(wakes_hands_free(Some(gateway)));
        assert!(wakes_hands_free(Some("0000111F-0000-1000-8000-00805F9B34FB")));
        assert!(!wakes_hands_free(Some("00000000-deca-fade-deca-deafdecacafe")));
        assert!(!wakes_hands_free(None));
        assert_eq!(service_element(Some(gateway)), sdp::uuid_of(sdp::HANDS_FREE_GATEWAY));
        let iap = sdp::uuid_of_128(&sdp::IAP_CLIENT_UUID);
        assert_eq!(service_element(Some("00000000-deca-fade-deca-deafdecacafe")), iap);
        assert_eq!(service_element(None), iap);
        assert_eq!(service_element(Some("not a uuid")), iap);
    }
}
