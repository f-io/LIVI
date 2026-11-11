// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashSet;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use livi_bt_host::sdp::{self, Service};
use livi_bt_host::{Call, Config, Inbox, Incoming, Notice, Stack, hci};
use livi_helper_stack::bringup::OnCable;
use livi_helper_stack::hfp::Hfp;
use livi_helper_stack::livi_sock::Broadcaster;
use livi_helper_stack::reconnect::{self, Link, PING_TIMEOUT, Paging};
use livi_helper_stack::sco::ScoSink;
use livi_helper_stack::state::HelperState;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use crate::link::LinkPresence;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY: Duration = Duration::from_secs(5);

pub struct Arrival {
    pub peer: String,
    pub local: [u8; 6],
    pub stream: tokio::net::UnixStream,
}

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
        if stack.disconnect(&addr) {
            Ok(())
        } else {
            Err(format!("{mac} is not connected"))
        }
    }

    pub fn end(&self) {
        if let Some(stack) = self.0.lock().unwrap().take() {
            stack.end();
        }
    }

    pub fn on_cable(&self, state: Arc<HelperState>) -> OnCable {
        let dongle = self.clone();
        OnCable::dropping(state, move |mac| {
            if let Err(e) = dongle.disconnect(mac) {
                eprintln!("[bt] {mac} stays on the dongle's bluetooth: {e}");
            }
        })
    }
}

pub struct Radio {
    pub name: String,
    pub keys: PathBuf,
    pub mic: PathBuf,
    pub offered: Vec<Service>,
}

fn open(name: String, keys: PathBuf) -> Result<(Stack, Inbox), String> {
    let host = livi_link_host::link::name().ok_or("the dongle has no address yet")?;
    let tunnel = livi_net::connect((host.as_str(), livi_net::port::HCI), CONNECT_TIMEOUT)
        .map_err(|e| format!("the dongle's controller: {e}"))?;
    let _ = tunnel.set_nodelay(true);
    Stack::start(tunnel, Config { name, keys }).map_err(|e| e.to_string())
}

pub async fn run(
    dongle: Dongle,
    link: Arc<LinkPresence>,
    profiles: Profiles,
    state: Arc<HelperState>,
    radio: Radio,
) {
    let mic = match livi_helper_stack::sco::mic_socket(&radio.mic) {
        Ok(mic) => Arc::new(mic),
        Err(e) => {
            eprintln!("[sco] mic socket failed: {e}");
            return;
        }
    };
    loop {
        link.wait_until(true).await;
        let (named, keys) = (radio.name.clone(), radio.keys.clone());
        let (stack, inbox) = match tokio::task::spawn_blocking(move || open(named, keys)).await {
            Ok(Ok(up)) => up,
            Ok(Err(e)) => {
                eprintln!("[bt] {e}");
                tokio::time::sleep(RETRY).await;
                continue;
            }
            Err(_) => return,
        };
        for service in &radio.offered {
            if let Err(e) = stack.offer(*service, true) {
                eprintln!("[bt] {e}");
            }
        }
        *dongle.0.lock().unwrap() = Some(stack.clone());
        let Inbox {
            incoming,
            calls,
            notices,
        } = inbox;
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
        tokio::spawn(reconnect::run(
            Pager {
                stack: stack.clone(),
                profiles: profiles.clone(),
                rt,
                busy: Arc::default(),
            },
            state.clone(),
        ));
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
            hand(
                rt,
                stack,
                profiles.android_auto.as_ref(),
                mac,
                arrived.stream,
            );
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
    let Ok(stream) = stream
        .set_nonblocking(true)
        .and_then(|()| tokio::net::UnixStream::from_std(stream))
    else {
        return;
    };
    let mut local = stack.local();
    local.reverse();
    let _ = to.send(Arrival {
        peer,
        local,
        stream,
    });
}

fn answer(call: Call, profiles: &Profiles, mic: &Arc<UnixListener>) {
    let link = livi_hfp::sco::Link {
        fd: OwnedFd::from(call.socket),
        mtu: livi_hfp::sco::MAX_PACKET,
        peer: call.peer,
    };
    let (events, sink, mic) = (
        profiles.events.clone(),
        profiles.sco_sink.clone(),
        mic.clone(),
    );
    std::thread::spawn(move || livi_helper_stack::sco::call(&events, &link, &mic, &sink));
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

fn wakes_hands_free(wake: Option<&str>) -> bool {
    wake.and_then(parse_uuid) == Some(sdp::full_uuid(sdp::HANDS_FREE_GATEWAY))
}

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
    let (mac, gateway, service) = (
        mac.to_string(),
        wakes_hands_free(wake),
        service_element(wake),
    );
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

struct Pager {
    stack: Stack,
    profiles: Profiles,
    rt: Handle,
    busy: Arc<Mutex<HashSet<String>>>,
}

impl Paging for Pager {
    fn alive(&self) -> bool {
        self.stack.alive()
    }

    fn ready(&self) -> bool {
        !self.profiles.hfp.in_call()
    }

    async fn link(&self, mac: &str) -> Link {
        match hci::parse_addr(mac) {
            Some(addr) if self.stack.connected(&addr) => Link::Up,
            Some(_) => Link::Down,
            None => Link::Unknown,
        }
    }

    async fn poke(&self, mac: &str, wake: Option<&str>) {
        if hci::parse_addr(mac).is_some_and(|addr| !self.stack.in_use(&addr)) {
            poke(&self.stack, &self.profiles, &self.rt, &self.busy, mac, wake);
        }
    }

    async fn page(&self, mac: &str, wake: Option<&str>) -> Option<bool> {
        let paging = poke(&self.stack, &self.profiles, &self.rt, &self.busy, mac, wake)?;
        println!("[bt] paging {mac}");
        Some(matches!(
            tokio::time::timeout(PING_TIMEOUT, paging).await,
            Ok(Ok(true))
        ))
    }

    async fn disconnect(&self, mac: &str) {
        if let Some(addr) = hci::parse_addr(mac) {
            self.stack.disconnect(&addr);
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
        assert!(wakes_hands_free(Some(
            "0000111F-0000-1000-8000-00805F9B34FB"
        )));
        assert!(!wakes_hands_free(Some(
            "00000000-deca-fade-deca-deafdecacafe"
        )));
        assert!(!wakes_hands_free(None));
        assert_eq!(
            service_element(Some(gateway)),
            sdp::uuid_of(sdp::HANDS_FREE_GATEWAY)
        );
        let iap = sdp::uuid_of_128(&sdp::IAP_CLIENT_UUID);
        assert_eq!(
            service_element(Some("00000000-deca-fade-deca-deafdecacafe")),
            iap
        );
        assert_eq!(service_element(None), iap);
        assert_eq!(service_element(Some("not a uuid")), iap);
    }
}
