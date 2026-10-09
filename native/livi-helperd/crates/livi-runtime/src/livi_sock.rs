use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc, watch};

use iap2_link::LinkConfig;

use crate::bringup::{BringupEvent, CpConfig, run_accessory};
use crate::driver::spawn_link;
use crate::ident::Identity;
use crate::state::HelperState;
use crate::vehicle::VehicleFeed;
use crate::{AsyncAuth, events};

pub const SOCK_PATH: &str = "/tmp/cp-bt.sock";

#[derive(Clone, Default)]
pub struct Broadcaster {
    subs: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>,
    joined: Arc<Notify>,
}

impl Broadcaster {
    pub fn push_json(&self, line: String) {
        self.subs.lock().unwrap().retain(|tx| tx.send(line.clone()).is_ok());
    }

    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subs.lock().unwrap().push(tx);
        self.joined.notify_waiters();
        rx
    }

    /// A line pushed before the first subscriber reaches nobody.
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

/// Wired CarPlay needs none of it, so the socket serves before it is there.
#[derive(Clone)]
pub struct Bluez {
    pub bus: zbus::Connection,
    pub adapter: String,
    pub bt_mac: [u8; 6],
}

#[derive(Clone)]
pub struct LiviSockConfig {
    pub path: String,
    pub identity: Identity,
    pub cp: CpConfig,
    /// Used where there is no BlueZ.
    pub disconnect: Option<DropLink>,
    /// Used where there is no BlueZ.
    pub targets: Option<PushTargets>,
    /// Takes the place of `cp`. The access point can change its MAC, SSID and channel, and a
    /// stale `device_identifier` makes the phone drop the session.
    pub cp_live: Option<CpFactory>,
}

pub type DropLink = Arc<dyn Fn(String) -> Result<(), String> + Send + Sync>;

pub type CpFactory = Arc<dyn Fn() -> CpConfig + Send + Sync>;

/// In paging order, each phone with the profile that wakes it.
pub type PushTargets =
    Arc<dyn Fn(Vec<(String, Option<String>)>) -> Result<(), String> + Send + Sync>;

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
    let _ = std::fs::remove_file(&cfg.path);
    let listener = UnixListener::bind(&cfg.path)?;
    std::fs::set_permissions(&cfg.path, std::fs::Permissions::from_mode(0o666))?;
    println!("[cp-sock] listening on {}", cfg.path);

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

async fn read_header(stream: &mut UnixStream) -> io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let b = stream.read_u8().await?;
        if b == b'\n' {
            break;
        }
        buf.push(b);
        if buf.len() > 4096 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

async fn reply(stream: &mut UnixStream, json: &str) -> io::Result<()> {
    stream.write_all(json.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await
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
    let line = read_header(&mut stream).await?;
    let (verb, arg) = match line.split_once(' ') {
        Some((v, a)) => (v, a.trim()),
        None => (line.as_str(), ""),
    };

    match verb {
        "subscribe" => run_subscriber(stream, bcast).await,
        "tunnel" => {
            // "tunnel <cid> [btMac]", the MAC is told apart by its colons.
            let (cid, bt_mac) = match arg.rsplit_once(' ') {
                Some((c, m)) if m.contains(':') => (c.trim(), m),
                _ => (arg, ""),
            };
            if state.carkit_blocks(bt_mac) {
                println!(
                    "[cp-sock] tunnel refused, iAP2 already runs over USB carkit (cid={cid}, btMac={})",
                    if bt_mac.is_empty() { "unknown" } else { bt_mac }
                );
                return Ok(());
            }
            println!(
                "[cp-sock] tunnel up (cid={cid}, btMac={})",
                if bt_mac.is_empty() { "unknown" } else { bt_mac }
            );
            let cfg = match &bluez {
                Some(bluez) => LiviSockConfig {
                    identity: Identity { bt_mac: bluez.bt_mac, ..cfg.identity },
                    ..cfg
                },
                None => cfg,
            };
            run_tunnel(stream, auth, cfg, bcast, cid.to_string(), state.vehicle_feed());
            Ok(())
        }
        "certificate" => {
            // An unknown generation is an error, not a guess.
            let json = match (auth.read_certificate().await, auth.protocol_major().await) {
                (Ok(cert), Ok(major)) => format!(
                    "{{\"ok\":true,\"data\":\"{}\",\"protocolMajor\":{}}}",
                    STANDARD.encode(&cert),
                    major
                ),
                (Err(e), _) | (_, Err(e)) => err_json(&format!("MFi: {e}")),
            };
            reply(&mut stream, &json).await
        }
        "sign" => {
            let json = match STANDARD.decode(arg) {
                Ok(digest) => match auth.sign(digest).await {
                    Ok(sig) => format!("{{\"ok\":true,\"data\":\"{}\"}}", STANDARD.encode(&sig)),
                    Err(e) => err_json(&e),
                },
                Err(e) => err_json(&format!("bad base64: {e}")),
            };
            reply(&mut stream, &json).await
        }
        "disconnect" => {
            let json = if arg.is_empty() {
                err_json("disconnect requires a MAC")
            } else {
                match bluez.as_ref() {
                    Some(bluez) => match device_disconnect(&bluez.bus, &bluez.adapter, arg).await {
                        Ok(()) => "{\"ok\":true}".to_string(),
                        Err(e) => err_json(&e),
                    },
                    None => match cfg.disconnect.clone() {
                        Some(drop) => {
                            let mac = arg.to_string();
                            match tokio::task::spawn_blocking(move || drop(mac)).await {
                                Ok(Ok(())) => "{\"ok\":true}".to_string(),
                                Ok(Err(e)) => err_json(&e),
                                Err(e) => err_json(&e.to_string()),
                            }
                        }
                        None => err_json("nothing here can drop a bluetooth link"),
                    },
                }
            };
            reply(&mut stream, &json).await
        }
        "reconnect-targets" => {
            let json = match parse_reconnect_targets(arg) {
                Ok(targets) => {
                    // LIVI refreshes this once a second, so logging every call is noise.
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
                    "{\"ok\":true}".to_string()
                }
                Err(e) => err_json(&e),
            };
            reply(&mut stream, &json).await
        }
        "location" => {
            let json = match state.vehicle().push_location(arg) {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => err_json(&e),
            };
            reply(&mut stream, &json).await
        }
        "vehicle-status" => {
            let json = match state.vehicle().push_status(arg) {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => err_json(&e),
            };
            reply(&mut stream, &json).await
        }
        "seek" => {
            let json = match state.vehicle().push_seek(arg) {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => err_json(&e),
            };
            reply(&mut stream, &json).await
        }
        "drop-iap2" => {
            let n = state.restart_wired();
            println!("[cp-sock] drop-iap2: {n} wired session(s) end for a fresh start");
            reply(&mut stream, "{\"ok\":true}").await
        }
        "start-wired" => {
            let json = if state.start_wired_again(arg) {
                println!("[cp-sock] start-wired: {arg} offers CarPlay again");
                "{\"ok\":true}".to_string()
            } else {
                err_json(&format!("no wired session for {arg:?}"))
            };
            reply(&mut stream, &json).await
        }
        other => reply(&mut stream, &err_json(&format!("unknown command: {other}"))).await,
    }
}

// [["MAC", "uuid" | null], ...]. The array order is the paging order.
fn parse_reconnect_targets(arg: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let value: serde_json::Value = serde_json::from_str(arg).map_err(|e| e.to_string())?;
    let list = value.as_array().ok_or("reconnect-targets expects a JSON array")?;
    list.iter()
        .map(|pair| {
            let mac = pair
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or("reconnect-targets entries are [mac, uuid] pairs")?;
            let uuid = pair.get(1).and_then(|v| v.as_str()).map(str::to_string);
            Ok((mac.to_string(), uuid))
        })
        .collect()
}

fn err_json(msg: &str) -> String {
    format!("{{\"ok\":false,\"error\":\"{}\"}}", msg.replace('"', "'"))
}

async fn run_subscriber(mut stream: UnixStream, bcast: Broadcaster) -> io::Result<()> {
    let mut rx = bcast.subscribe();
    let mut probe = [0u8; 64];
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(line) => {
                    stream.write_all(line.as_bytes()).await?;
                    stream.write_all(b"\n").await?;
                    stream.flush().await?;
                }
                None => return Ok(()),
            },
            read = stream.read(&mut probe) => {
                if read? == 0 {
                    return Ok(());
                }
            }
        }
    }
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
    let link_cfg =
        LinkConfig { max_outgoing: 4, control_version: 2, zero_ack: true, ..LinkConfig::default() };
    let (channel, art_rx) = spawn_link(fd, link_cfg, true);
    let (tx, rx) = mpsc::channel(64);
    let cp = match &cfg.cp_live {
        Some(resolve) => resolve(),
        None => cfg.cp,
    };
    tokio::spawn(run_accessory(channel, auth, cfg.identity, cp, tx, vehicle));
    let ident: SharedTag = Arc::new(Mutex::new(events::EventTag {
        cid: (!cid.is_empty()).then_some(cid),
        ..Default::default()
    }));
    tokio::spawn(pump_events_for(rx, bcast.clone(), "tunnel", None, ident.clone()));
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
        let json = format!("{{\"type\":\"albumart\",\"dataB64\":\"{}\"}}", STANDARD.encode(&data));
        bcast.push_json(ident.lock().unwrap().apply(json));
    }
}

pub type SharedTag = Arc<Mutex<events::EventTag>>;

/// `usb_udid` marks a wired session.
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
                    crate::clock::step_to(secs);
                }
                let tagged = {
                    let mut t = ident.lock().unwrap();
                    t.learn(&frame);
                    (
                        events::device_json(&frame, usb_udid.as_deref()).map(|j| t.apply(j)),
                        events::to_json(&frame)
                            .or_else(|| t.navigation_json(&frame))
                            .map(|j| t.apply(j)),
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
                    let json = events::wired_start_json(udid, &ip);
                    bcast.push_json(ident.lock().unwrap().apply(json));
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
) -> Result<(), String> {
    let path = format!("/org/bluez/{}/dev_{}", adapter, mac.replace(':', "_").to_uppercase());
    bus.call_method(Some("org.bluez"), path.as_str(), Some("org.bluez.Device1"), "Disconnect", &())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
