// /tmp/cp-bt.sock: line-JSON RPC for MFi and device control, an event stream, and a raw
// tunnel that carries an iAP2 session over CarPlay Wi-Fi.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use iap2_link::LinkConfig;

use crate::bringup::{run_accessory, BringupEvent, CpConfig};
use crate::driver::spawn_link;
use crate::ident::Identity;
use crate::state::HelperState;
use crate::{events, AsyncAuth};

pub const SOCK_PATH: &str = "/tmp/cp-bt.sock";

/// Fan-out of JSON event lines to every connected `subscribe` client.
#[derive(Clone, Default)]
pub struct Broadcaster {
    subs: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>,
}

impl Broadcaster {
    pub fn push_json(&self, line: String) {
        self.subs.lock().unwrap().retain(|tx| tx.send(line.clone()).is_ok());
    }

    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subs.lock().unwrap().push(tx);
        rx
    }
}

#[derive(Clone)]
pub struct LiviSockConfig {
    pub path: String,
    pub adapter: String,
    pub identity: Identity,
    pub cp: CpConfig,
}

pub async fn serve<A>(
    cfg: LiviSockConfig,
    auth: A,
    bus: Option<zbus::Connection>,
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
        let bus = bus.clone();
        let bcast = bcast.clone();
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, auth, cfg, bus, bcast, state).await {
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
    bus: Option<zbus::Connection>,
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
            // "tunnel <cid> [btMac]" — the MAC is recognizable by its colons.
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
            println!("[cp-sock] tunnel up (cid={cid}, btMac={})", if bt_mac.is_empty() { "unknown" } else { bt_mac });
            run_tunnel(stream, auth, cfg, bcast, cid.to_string());
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
                match bus.as_ref() {
                    Some(bus) => match device_disconnect(bus, &cfg.adapter, arg).await {
                        Ok(()) => "{\"ok\":true}".to_string(),
                        Err(e) => err_json(&e),
                    },
                    None => err_json("disconnect unavailable without BlueZ"),
                }
            };
            reply(&mut stream, &json).await
        }
        "reconnect-targets" => {
            let json = match parse_reconnect_targets(arg) {
                Ok(targets) => {
                    // LIVI refreshes this once a second, so logging every call is noise.
                    state.set_reconnect_targets(targets);
                    "{\"ok\":true}".to_string()
                }
                Err(e) => err_json(&e),
            };
            reply(&mut stream, &json).await
        }
        // Profiles are registered at startup and stay up; the toggles are accepted no-ops.
        "set-cp" | "set-aa" => reply(&mut stream, "{\"ok\":true}").await,
        // Injected into the active iAP2 session when one exists; dropped otherwise.
        "vehicle-status" | "location" | "drop-iap2" => reply(&mut stream, "{\"ok\":true}").await,
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

fn run_tunnel<A>(stream: UnixStream, auth: A, cfg: LiviSockConfig, bcast: Broadcaster, cid: String)
where
    A: AsyncAuth + Clone + Send + 'static,
{
    let Ok(std_stream) = stream.into_std() else {
        return;
    };
    let fd: OwnedFd = std_stream.into();
    let link_cfg = LinkConfig { max_outgoing: 4, control_version: 2, zero_ack: true, ..LinkConfig::default() };
    let (channel, art_rx) = spawn_link(fd, link_cfg, true);
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(run_accessory(channel, auth, cfg.identity, cfg.cp, tx));
    let ident: SharedTag = Arc::new(Mutex::new(events::EventTag {
        cid: (!cid.is_empty()).then_some(cid),
        ..Default::default()
    }));
    tokio::spawn(pump_events_for(rx, bcast.clone(), "tunnel", None, ident.clone()));
    tokio::spawn(pump_artwork(art_rx, bcast, ident));
}

/// Forwards completed album artwork to the UI as base64 albumart events.
pub async fn pump_artwork(mut art_rx: crate::driver::ArtworkRx, bcast: Broadcaster, ident: SharedTag) {
    while let Some(data) = art_rx.recv().await {
        if data.is_empty() {
            continue;
        }
        let json = format!("{{\"type\":\"albumart\",\"dataB64\":\"{}\"}}", STANDARD.encode(&data));
        bcast.push_json(ident.lock().unwrap().apply(json));
    }
}

pub type SharedTag = Arc<Mutex<events::EventTag>>;

pub async fn pump_events(rx: mpsc::Receiver<BringupEvent>, bcast: Broadcaster, tag: &'static str) {
    pump_events_for(rx, bcast, tag, None, Arc::new(Mutex::new(events::EventTag::default()))).await
}

/// Forwards bring-up telemetry to the UI: decodes incoming CSM into JSON and broadcasts it.
/// `usb_udid` marks a wired session; every metadata event carries the phone's iAP2 identity.
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
                if !time_synced
                    && let Some(secs) = events::device_time(&frame) {
                        time_synced = true;
                        crate::clock::step_to(secs);
                    }
                let tagged = {
                    let mut t = ident.lock().unwrap();
                    t.learn(&frame);
                    (
                        events::device_json(&frame, usb_udid.as_deref()).map(|j| t.apply(j)),
                        events::to_json(&frame).map(|j| t.apply(j)),
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
            BringupEvent::CarPlayStartSent => println!("[cp] {tag}: CarPlayStartSession sent"),
            _ => {}
        }
    }
}

async fn device_disconnect(
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
