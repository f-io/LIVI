// CarPlay mDNS: publishes the AirPlay receiver and browses the phone's _carplay-ctrl.
// Browse/resolve run on avahi on Linux and on dns-sd on macOS.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::livi_sock::Broadcaster;

const AIRPLAY_SERVICE: &str = "_airplay._tcp";
const CARPLAY_CTRL: &str = "_carplay-ctrl._tcp";

pub struct Bonjour {
    _publisher: Child,
    device_id: String,
    source_version: String,
    bcast: Broadcaster,
    seen: Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
}

fn txt_records(device_id: &str, source_version: &str, pk: &str, pi: &str) -> Vec<String> {
    let mut txt = vec![
        format!("deviceid={device_id}"),
        "features=0x44540380,0x61".to_string(),
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

impl Bonjour {
    pub fn start(
        device_id: String,
        airplay_port: u16,
        source_version: String,
        pk: String,
        pi: String,
        bcast: Broadcaster,
    ) -> std::io::Result<Self> {
        let txt = txt_records(&device_id, &source_version, &pk, &pi);
        // macOS advertises through mDNSResponder (dns-sd); Linux through avahi.
        #[cfg(target_os = "macos")]
        let publisher = {
            let mut args = vec![
                "-R".to_string(),
                "LIVI".to_string(),
                AIRPLAY_SERVICE.to_string(),
                ".".to_string(),
                airplay_port.to_string(),
            ];
            args.extend(txt);
            Command::new("dns-sd")
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
        };
        #[cfg(not(target_os = "macos"))]
        let publisher = {
            let mut args = vec![
                "LIVI".to_string(),
                AIRPLAY_SERVICE.to_string(),
                airplay_port.to_string(),
            ];
            args.extend(txt);
            Command::new("avahi-publish-service")
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
        };
        println!("[cp] published {AIRPLAY_SERVICE} port={airplay_port}");

        let bonjour = Self {
            _publisher: publisher,
            device_id,
            source_version,
            bcast,
            seen: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        bonjour.spawn_browser();
        Ok(bonjour)
    }

    fn spawn_browser(&self) {
        let device_id = self.device_id.clone();
        let source_version = self.source_version.clone();
        let bcast = self.bcast.clone();
        let seen = self.seen.clone();
        tokio::spawn(async move {
            loop {
                #[cfg(target_os = "macos")]
                {
                    let (d, s, b, sn) =
                        (device_id.clone(), source_version.clone(), bcast.clone(), seen.clone());
                    let _ = tokio::task::spawn_blocking(move || macos_browse_once(&d, &s, &b, &sn)).await;
                }
                #[cfg(not(target_os = "macos"))]
                if let Err(e) = browse_once(&device_id, &source_version, &bcast, &seen).await {
                    eprintln!("[cp] carplay-ctrl browse ended: {e}");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

async fn browse_once(
    device_id: &str,
    source_version: &str,
    bcast: &Broadcaster,
    seen: &Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
) -> std::io::Result<()> {
    let mut child = Command::new("avahi-browse")
        .args(["-r", "-p", "-k", CARPLAY_CTRL])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        if !line.starts_with('=') {
            continue;
        }
        let Some(ep) = parse_resolved(&line) else { continue };
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
        println!("[cp] found phone {CARPLAY_CTRL} at {}:{}", ep.address, ep.port);
        if let Some(mac) = &ep.phone_bt {
            let ip = ep.address.split('%').next().unwrap_or(&ep.address);
            bcast.push_json(format!(
                "{{\"type\":\"device\",\"src\":\"bonjour\",\"btMac\":\"{mac}\",\"ip\":\"{ip}\"}}"
            ));
        }
        let device_id = device_id.to_string();
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

    Some(Endpoint { iface, address, port, phone_bt })
}

fn connect_probe(ep: &Endpoint, device_id: &str, source_version: &str) {
    let mac_int = device_id.replace(':', "");
    let host = ep.address.split('%').next().unwrap_or(&ep.address);
    let is_v6 = ep.address.contains(':');
    let host_hdr = if is_v6 { format!("[{host}]:{}", ep.port) } else { format!("{host}:{}", ep.port) };
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
        let scope = ep.address.split('%').nth(1).and_then(nametoindex).unwrap_or(0);
        (Domain::IPV6, SocketAddr::V6(SocketAddrV6::new(ip, ep.port, 0, scope)))
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
    let idx = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if idx == 0 {
        None
    } else {
        Some(idx)
    }
}

fn io_err(msg: &str) -> std::io::Error {
    std::io::Error::other(msg)
}

// --- macOS browse via dns-sd: one pass finds the phone's _carplay-ctrl instances, resolves
// each to a link-local and connect-probes it. ---

#[cfg(target_os = "macos")]
fn macos_browse_once(
    device_id: &str,
    source_version: &str,
    bcast: &Broadcaster,
    seen: &Arc<Mutex<std::collections::HashMap<String, (String, u16)>>>,
) {
    for inst in dns_sd_browse(CARPLAY_CTRL) {
        let Some(ep) = dns_sd_resolve(&inst) else { continue };
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
        println!("[cp] found phone {CARPLAY_CTRL} at {}:{}", ep.address, ep.port);
        if let Some(mac) = &ep.phone_bt {
            let ip = ep.address.split('%').next().unwrap_or(&ep.address);
            bcast.push_json(format!(
                "{{\"type\":\"device\",\"src\":\"bonjour\",\"btMac\":\"{mac}\",\"ip\":\"{ip}\"}}"
            ));
        }
        connect_probe(&ep, device_id, source_version);
    }
}

/// Run `dns-sd <args>` for `secs` seconds, then terminate it and return the output lines.
#[cfg(target_os = "macos")]
fn dns_sd_run(args: &[&str], secs: u64) -> Vec<String> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let mut out = Vec::new();
    let Ok(mut child) =
        Command::new("dns-sd").args(args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    else {
        return out;
    };
    if let Some(stdout) = child.stdout.take() {
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(secs));
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        });
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            out.push(line);
        }
    }
    let _ = child.wait();
    out
}

#[cfg(target_os = "macos")]
fn dns_sd_browse(service: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in dns_sd_run(&["-B", service], 4) {
        if !line.contains(" Add ") {
            continue;
        }
        // "... Add <flags> <if> <domain> _carplay-ctrl._tcp. <Instance Name>"
        if let Some(pos) = line.find("_tcp.") {
            let inst = line[pos + "_tcp.".len()..].trim().to_string();
            if !inst.is_empty() && !names.contains(&inst) {
                names.push(inst);
            }
        }
    }
    names
}

#[cfg(target_os = "macos")]
fn dns_sd_resolve(instance: &str) -> Option<Endpoint> {
    use std::net::ToSocketAddrs;
    let mut host = String::new();
    let mut port = 0u16;
    let mut phone_bt = None;
    for line in dns_sd_run(&["-L", instance, CARPLAY_CTRL], 3) {
        if let Some(pos) = line.find("can be reached at ") {
            // "<host>.:<port> (interface N)"
            let token = line[pos + "can be reached at ".len()..].split_whitespace().next().unwrap_or("");
            if let Some(colon) = token.rfind(':') {
                host = token[..colon].trim_end_matches('.').to_string();
                port = token[colon + 1..].parse().unwrap_or(0);
            }
        }
        if phone_bt.is_none()
            && let Some(id) = line.split(&['"', ' '][..]).find_map(|t| t.strip_prefix("id="))
        {
            phone_bt = Some(id.trim().to_lowercase());
        }
    }
    if host.is_empty() || port == 0 {
        return None;
    }
    // getaddrinfo resolves the .local name (via mDNSResponder) to the phone's link-local.
    let sa = format!("{host}:{port}")
        .to_socket_addrs()
        .ok()?
        .find(|a| matches!(a, SocketAddr::V6(v6) if (v6.ip().segments()[0] & 0xffc0) == 0xfe80))?;
    let SocketAddr::V6(v6) = sa else { return None };
    let iface = ifname_from_index(v6.scope_id()).unwrap_or_default();
    let address =
        if iface.is_empty() { v6.ip().to_string() } else { format!("{}%{}", v6.ip(), iface) };
    Some(Endpoint { iface, address, port, phone_bt })
}

#[cfg(target_os = "macos")]
fn ifname_from_index(idx: u32) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let mut buf = [0u8; libc::IF_NAMESIZE];
    let p = unsafe { libc::if_indextoname(idx, buf.as_mut_ptr() as *mut libc::c_char) };
    if p.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_str().ok().map(|s| s.to_string())
}
