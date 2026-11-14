// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#![cfg_attr(target_vendor = "apple", allow(dead_code))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use livi_core_proto::features::{self, CARPLAY_FEATURES};
use livi_core_proto::helper::Mirror;
use livi_core_proto::helper_events::{DeviceSource, HelperEvent, HelperKind};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

use crate::livi_sock::Broadcaster;

pub const AIRPLAY_SERVICE: &str = "_airplay._tcp";
const CARPLAY_CTRL: &str = "_carplay-ctrl._tcp";

#[cfg(not(target_vendor = "apple"))]
type Publisher = tokio::process::Child;
#[cfg(target_vendor = "apple")]
type Publisher = crate::dnssd::Registration;

pub struct Bonjour {
    publisher: Arc<Mutex<Option<Publisher>>>,
    mirror: Option<Publisher>,
    device_id: Arc<Mutex<String>>,
    source_version: String,
    bcast: Broadcaster,
    seen: Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
}

#[derive(Clone)]
struct Record {
    airplay_port: u16,
    source_version: String,
    pk: String,
    pi: String,
}

static NAMED: LazyLock<watch::Sender<Option<String>>> = LazyLock::new(|| watch::Sender::new(None));

pub fn named(device_id: &str) {
    NAMED.send_if_modified(|named| {
        let changed = named.as_deref() != Some(device_id);
        if changed {
            *named = Some(device_id.to_string());
        }
        changed
    });
}

fn receiver_id(device_id: &str) -> Option<u64> {
    let hex = device_id.replace(':', "");
    (hex.len() == 12)
        .then(|| u64::from_str_radix(&hex, 16).ok())
        .flatten()
}

pub fn txt_records(device_id: &str, source_version: &str, pk: &str, pi: &str) -> Vec<String> {
    let mut txt = vec![
        format!("deviceid={device_id}"),
        format!("features={}", features::txt(CARPLAY_FEATURES)),
        "flags=0x4".to_string(),
        "model=LIVI".to_string(),
        format!("srcvers={source_version}"),
        "protovers=1.1".to_string(),
    ];
    if !pi.is_empty() {
        txt.push(format!("pi={pi}"));
    }
    if !pk.is_empty() {
        txt.push(format!("pk={pk}"));
    }
    txt
}

#[cfg(not(target_vendor = "apple"))]
fn reap(pattern: &str) {
    let _ = std::process::Command::new(livi_base::sys::tool("pkill"))
        .args(["-f", pattern])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(target_vendor = "apple"))]
impl Drop for Bonjour {
    fn drop(&mut self) {
        if let Some(mut publisher) = self.publisher.lock().unwrap().take() {
            let _ = publisher.start_kill();
        }
        if let Some(mirror) = self.mirror.as_mut() {
            let _ = mirror.start_kill();
        }
    }
}

fn publish(record: &Record, device_id: &str) -> std::io::Result<Publisher> {
    let txt = txt_records(device_id, &record.source_version, &record.pk, &record.pi);
    spawn("LIVI", record.airplay_port, &txt)
}

#[cfg(target_vendor = "apple")]
fn spawn(name: &str, port: u16, txt: &[String]) -> std::io::Result<Publisher> {
    crate::dnssd::register(name, AIRPLAY_SERVICE, port, txt)
}

#[cfg(not(target_vendor = "apple"))]
fn spawn(name: &str, port: u16, txt: &[String]) -> std::io::Result<Publisher> {
    let port = port.to_string();
    let mut args = vec![name, AIRPLAY_SERVICE, port.as_str()];
    args.extend(txt.iter().map(String::as_str));
    Command::new(livi_base::sys::tool("avahi-publish-service"))
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(target_vendor = "apple")]
async fn retire(publisher: Publisher) {
    drop(publisher);
}

#[cfg(not(target_vendor = "apple"))]
async fn retire(mut publisher: Publisher) {
    let _ = publisher.kill().await;
}

impl Bonjour {
    pub fn start(
        device_id: String,
        airplay_port: u16,
        source_version: String,
        pk: String,
        pi: String,
        mirror: Option<Mirror>,
        bcast: Broadcaster,
    ) -> std::io::Result<Self> {
        #[cfg(not(target_vendor = "apple"))]
        reap(&format!("avahi-publish-service.*{AIRPLAY_SERVICE}"));
        let record = Record {
            airplay_port,
            source_version: source_version.clone(),
            pk,
            pi,
        };
        let publisher = publish(&record, &device_id)?;
        println!("[cp] published {AIRPLAY_SERVICE} port={airplay_port} deviceid={device_id}");
        let mirror = mirror.and_then(|m| match spawn(&m.name, m.port, &m.txt) {
            Ok(child) => {
                println!(
                    "[airplay] published {AIRPLAY_SERVICE} {:?} port={}",
                    m.name, m.port
                );
                Some(child)
            }
            Err(e) => {
                eprintln!(
                    "[airplay] {AIRPLAY_SERVICE} {:?} not published: {e}",
                    m.name
                );
                None
            }
        });

        let bonjour = Self {
            publisher: Arc::new(Mutex::new(Some(publisher))),
            mirror,
            device_id: Arc::new(Mutex::new(device_id)),
            source_version,
            bcast,
            seen: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        bonjour.follow(record);
        bonjour.spawn_browser();
        Ok(bonjour)
    }

    fn follow(&self, record: Record) {
        let (publisher, device_id) = (self.publisher.clone(), self.device_id.clone());
        let mut named = NAMED.subscribe();
        tokio::spawn(async move {
            loop {
                let wanted = named.borrow_and_update().clone();
                if let Some(id) = wanted.filter(|id| *device_id.lock().unwrap() != *id) {
                    let old = publisher.lock().unwrap().take();
                    if let Some(old) = old {
                        retire(old).await;
                    }
                    match publish(&record, &id) {
                        Ok(new) => {
                            *publisher.lock().unwrap() = Some(new);
                            println!("[cp] published {AIRPLAY_SERVICE} again, deviceid={id}");
                            *device_id.lock().unwrap() = id;
                        }
                        Err(e) => eprintln!("[cp] {AIRPLAY_SERVICE} not published again: {e}"),
                    }
                }
                if named.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    fn spawn_browser(&self) {
        let device_id = self.device_id.clone();
        let source_version = self.source_version.clone();
        let bcast = self.bcast.clone();
        let seen = self.seen.clone();
        tokio::spawn(async move {
            loop {
                #[cfg(target_vendor = "apple")]
                {
                    let (d, s, b, sn) = (
                        device_id.clone(),
                        source_version.clone(),
                        bcast.clone(),
                        seen.clone(),
                    );
                    let _ = tokio::task::spawn_blocking(move || apple_browse_once(&d, &s, &b, &sn))
                        .await;
                }
                #[cfg(not(target_vendor = "apple"))]
                if let Err(e) = browse_once(&device_id, &source_version, &bcast, &seen).await {
                    eprintln!("[cp] carplay-ctrl browse ended: {e}");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

async fn browse_once(
    device_id: &Mutex<String>,
    source_version: &str,
    bcast: &Broadcaster,
    seen: &Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
) -> std::io::Result<()> {
    let mut child = Command::new(livi_base::sys::tool("avahi-browse"))
        .args(["-r", "-p", "-k", CARPLAY_CTRL])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        if !line.starts_with('=') {
            continue;
        }
        let Some(ep) = parse_resolved(&line) else {
            continue;
        };
        let key = ep.phone_bt.clone().unwrap_or_else(|| ep.address.clone());
        let endpoint = (ep.address.clone(), ep.port);
        let fresh = {
            let mut s = seen.lock().unwrap();
            if s.get(&key) == Some(&endpoint) {
                false
            } else {
                s.insert(key.clone(), endpoint.clone());
                true
            }
        };
        if !fresh {
            continue;
        }
        println!(
            "[cp] found phone {CARPLAY_CTRL} at {}:{}",
            ep.address, ep.port
        );
        if let Some(mac) = &ep.phone_bt {
            let ip = ep.address.split('%').next().unwrap_or(&ep.address);
            bcast.push_json(seen_over_bonjour(mac, ip));
        }
        let device_id = device_id.lock().unwrap().clone();
        let source_version = source_version.to_string();
        tokio::task::spawn_blocking(move || connect_probe(&ep, &device_id, &source_version));
    }
    Ok(())
}

struct Endpoint {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    iface: String,
    address: String,
    port: u16,
    phone_bt: Option<String>,
}

fn parse_resolved(line: &str) -> Option<Endpoint> {
    let f: Vec<&str> = line.split(';').collect();
    if f.len() < 10 {
        return None;
    }
    let iface = f[1].to_string();
    let proto = f[2];
    let mut address = f[7].to_string();
    let port: u16 = f[8].parse().ok()?;
    let txt = f[9];

    let is_v6 = proto == "IPv6" || address.contains(':');
    if is_v6 {
        if !address.to_lowercase().starts_with("fe80") {
            return None;
        }
        if !iface.is_empty() && !address.contains('%') {
            address = format!("{address}%{iface}");
        }
    } else if address.starts_with("169.254.") {
        return None;
    }

    let phone_bt = txt
        .split(&['"', ' '][..])
        .find_map(|t| t.strip_prefix("id="))
        .map(|s| s.trim().to_lowercase());

    Some(Endpoint {
        iface,
        address,
        port,
        phone_bt,
    })
}

fn connect_probe(ep: &Endpoint, device_id: &str, source_version: &str) {
    let Some(mac_int) = receiver_id(device_id) else {
        println!("[cp] /ctrl-int/1/connect skipped, {device_id:?} is no MAC");
        return;
    };
    let host = ep.address.split('%').next().unwrap_or(&ep.address);
    let is_v6 = ep.address.contains(':');
    let host_hdr = if is_v6 {
        format!("[{host}]:{}", ep.port)
    } else {
        format!("{host}:{}", ep.port)
    };
    let req = format!(
        "GET /ctrl-int/1/connect HTTP/1.1\r\nHost: {host_hdr}\r\nUser-Agent: AirPlay/{source_version}\r\nAirPlay-Receiver-Device-ID: {mac_int}\r\nConnection: close\r\n\r\n"
    );

    for attempt in 1..=7 {
        match probe_attempt(ep, host, is_v6, req.as_bytes()) {
            Ok(first) => {
                println!("[cp] /ctrl-int/1/connect -> {first:?} (attempt {attempt})");
                return;
            }
            Err(e) => {
                if attempt == 7 {
                    println!("[cp] /ctrl-int/1/connect gave up: {e}");
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
    }
}

fn probe_attempt(ep: &Endpoint, host: &str, is_v6: bool, req: &[u8]) -> std::io::Result<String> {
    let (domain, addr): (Domain, SocketAddr) = if is_v6 {
        let ip: Ipv6Addr = host.parse().map_err(|_| io_err("bad v6"))?;
        let scope = ep
            .address
            .split('%')
            .nth(1)
            .and_then(nametoindex)
            .unwrap_or(0);
        (
            Domain::IPV6,
            SocketAddr::V6(SocketAddrV6::new(ip, ep.port, 0, scope)),
        )
    } else {
        let ip: Ipv4Addr = host.parse().map_err(|_| io_err("bad v4"))?;
        (Domain::IPV4, SocketAddr::V4(SocketAddrV4::new(ip, ep.port)))
    };

    let sock = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(target_os = "linux")]
    if !ep.iface.is_empty() {
        let _ = sock.bind_device(Some(ep.iface.as_bytes()));
    }
    sock.set_read_timeout(Some(Duration::from_secs(3)))?;
    sock.set_write_timeout(Some(Duration::from_secs(3)))?;
    sock.connect_timeout(&addr.into(), Duration::from_secs(3))?;

    let mut stream: std::net::TcpStream = sock.into();
    stream.write_all(req)?;
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Err(io_err("empty response"));
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    Ok(text.lines().next().unwrap_or("").to_string())
}

fn nametoindex(name: &str) -> Option<u32> {
    let cname = std::ffi::CString::new(name).ok()?;
    // SAFETY: cname is a valid NUL-terminated string for the call.
    let idx = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if idx == 0 { None } else { Some(idx) }
}

fn io_err(msg: &str) -> std::io::Error {
    std::io::Error::other(msg)
}

#[cfg(target_vendor = "apple")]
fn apple_browse_once(
    device_id: &Mutex<String>,
    source_version: &str,
    bcast: &Broadcaster,
    seen: &Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
) {
    for inst in dns_sd_browse(CARPLAY_CTRL) {
        let Some(ep) = dns_sd_resolve(&inst) else {
            continue;
        };
        let key = ep.phone_bt.clone().unwrap_or_else(|| ep.address.clone());
        let endpoint = (ep.address.clone(), ep.port);
        let fresh = {
            let mut s = seen.lock().unwrap();
            if s.get(&key) == Some(&endpoint) {
                false
            } else {
                s.insert(key.clone(), endpoint.clone());
                true
            }
        };
        if !fresh {
            continue;
        }
        println!(
            "[cp] found phone {CARPLAY_CTRL} at {}:{}",
            ep.address, ep.port
        );
        if let Some(mac) = &ep.phone_bt {
            let ip = ep.address.split('%').next().unwrap_or(&ep.address);
            bcast.push_json(seen_over_bonjour(mac, ip));
        }
        let device_id = device_id.lock().unwrap().clone();
        connect_probe(&ep, &device_id, source_version);
    }
}

#[cfg(target_vendor = "apple")]
fn dns_sd_browse(service: &str) -> Vec<String> {
    crate::dnssd::browse(service, Duration::from_secs(4))
}

#[cfg(target_vendor = "apple")]
fn dns_sd_resolve(instance: &str) -> Option<Endpoint> {
    let found = crate::dnssd::resolve(instance, CARPLAY_CTRL, 0, Duration::from_secs(3))?;
    let phone_bt = found
        .txt
        .iter()
        .find_map(|t| t.strip_prefix("id="))
        .map(|id| id.trim().to_lowercase());
    link_local(&found.host, found.port, phone_bt)
}

#[cfg(target_vendor = "apple")]
fn link_local(host: &str, port: u16, phone_bt: Option<String>) -> Option<Endpoint> {
    use std::net::ToSocketAddrs;
    let sa = format!("{host}:{port}")
        .to_socket_addrs()
        .ok()?
        .find(|a| matches!(a, SocketAddr::V6(v6) if (v6.ip().segments()[0] & 0xffc0) == 0xfe80))?;
    let SocketAddr::V6(v6) = sa else { return None };
    let iface = crate::dnssd::ifname(v6.scope_id()).unwrap_or_default();
    let address = if iface.is_empty() {
        v6.ip().to_string()
    } else {
        format!("{}%{}", v6.ip(), iface)
    };
    Some(Endpoint {
        iface,
        address,
        port,
        phone_bt,
    })
}

fn seen_over_bonjour(mac: &str, ip: &str) -> String {
    HelperEvent::from(HelperKind::Device {
        src: DeviceSource::Bonjour,
        bt_mac: Some(mac.to_string()),
        usb_udid: None,
        ip: Some(ip.to_string()),
        name: None,
    })
    .to_line()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_receiver_id_is_the_mac_as_a_number() {
        assert_eq!(receiver_id("11:22:33:44:55:66"), Some(18_838_586_676_582));
        assert_eq!(receiver_id("00:50:43:02:fc:01"), Some(0x0050_4302_fc01));
        assert_eq!(receiver_id("LIVI"), None);
        assert_eq!(receiver_id("zz:22:33:44:55:66"), None);
    }

    #[test]
    fn the_record_names_the_accessory() {
        let txt = txt_records("00:50:43:02:fc:01", "950.7.1", "", "pairing");
        assert_eq!(txt[0], "deviceid=00:50:43:02:fc:01");
        assert!(txt.contains(&"pi=pairing".to_string()));
        assert!(!txt.iter().any(|t| t.starts_with("pk=")));
    }

    #[test]
    fn naming_the_same_accessory_again_is_no_change() {
        let mut heard = NAMED.subscribe();
        named("02:00:00:00:00:01");
        assert!(heard.has_changed().unwrap());
        let _ = heard.borrow_and_update();
        named("02:00:00:00:00:01");
        assert!(!heard.has_changed().unwrap());
        assert_eq!(heard.borrow().as_deref(), Some("02:00:00:00:00:01"));
    }
}
