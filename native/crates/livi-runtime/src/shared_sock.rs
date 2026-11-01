// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use zbus::Connection;
use zbus::OwnedVariant;

use crate::livi_sock::Broadcaster;

pub const SOCK_PATH: &str = "/tmp/livi-shared.sock";

const WAKE_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";

type SetPlaybackStatus = Box<dyn Fn(&str) + Send + Sync>;

pub struct SharedSockDeps {
    pub adapter: String,
    pub wifi_iface: String,
    pub events: Broadcaster,
    pub set_playback_status: SetPlaybackStatus,
    pub deauth_dongle: Option<fn() -> Option<usize>>,
}

pub async fn serve(path: &str, bus: Option<Connection>, deps: SharedSockDeps) -> io::Result<()> {
    let listener = bind(path, "shared-sock")?;
    let deps = Arc::new(deps);
    loop {
        let (stream, _) = listener.accept().await?;
        let bus = bus.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, bus, deps).await {
                eprintln!("[shared-sock] connection error: {e}");
            }
        });
    }
}

pub(crate) fn bind(path: &str, name: &str) -> io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    println!("[{name}] listening on {path}");
    Ok(listener)
}

async fn handle(
    mut stream: UnixStream,
    bus: Option<Connection>,
    deps: Arc<SharedSockDeps>,
) -> io::Result<()> {
    let line = read_line(&mut stream).await?;
    let (verb, arg) = match line.split_once(' ') {
        Some((v, a)) => (v, a.trim()),
        None => (line.as_str(), ""),
    };

    if verb == "subscribe" {
        return run_subscriber(stream, deps.events.clone(), "shared-sock").await;
    }

    let json = match verb {
        "list_paired" | "connect" | "disconnect-profile" | "connect-full" | "disconnect"
        | "remove" => match bus.as_ref() {
            None => err_json("bluetooth unavailable on this platform"),
            Some(bus) => bluez_verb(bus, verb, arg, &deps.adapter).await,
        },
        "playback-status" => match arg {
            "playing" | "paused" | "stopped" => {
                (deps.set_playback_status)(arg);
                ok_json()
            }
            other => err_json(&format!("unknown playback status: {other}")),
        },
        "deauth-ap" => {
            let count = match deps.deauth_dongle {
                Some(deauth) => tokio::task::spawn_blocking(deauth)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(0),
                None => deauth_ap(&deps.wifi_iface).await,
            };
            println!("[shared-sock] deauth-ap: kicked {count} client(s)");
            format!("{{\"ok\":true,\"count\":{count}}}")
        }
        other => err_json(&format!("unknown command: {other}")),
    };
    reply(&mut stream, &json).await
}

async fn deauth_ap(iface: &str) -> usize {
    let cli = |args: &[&str]| {
        tokio::process::Command::new(crate::sys::tool("hostapd_cli"))
            .args(["-p", "/var/run/hostapd", "-i", iface])
            .args(args)
            .output()
    };
    let Ok(out) = cli(&["list_sta"]).await else {
        return 0;
    };
    let macs: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| l.len() == 17 && l.matches(':').count() == 5)
        .map(str::to_string)
        .collect();
    for mac in &macs {
        let _ = cli(&["deauthenticate", mac]).await;
    }
    macs.len()
}

pub(crate) async fn run_subscriber(
    mut stream: UnixStream,
    events: Broadcaster,
    name: &str,
) -> io::Result<()> {
    println!("[{name}] event subscriber connected");
    let mut rx = events.subscribe();
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
                    println!("[{name}] event subscriber gone");
                    return Ok(());
                }
            }
        }
    }
}

pub(crate) async fn reply(stream: &mut UnixStream, json: &str) -> io::Result<()> {
    stream.write_all(json.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await
}

pub(crate) fn ok_json() -> String {
    "{\"ok\":true}".to_string()
}

pub(crate) fn err_json(msg: &str) -> String {
    let clean = msg.replace(['"', '\n', '\r'], " ");
    format!("{{\"ok\":false,\"error\":\"{}\"}}", clean.trim())
}

fn action(result: Result<(), String>) -> String {
    match result {
        Ok(()) => ok_json(),
        Err(e) => err_json(&e),
    }
}

pub(crate) async fn read_line(stream: &mut UnixStream) -> io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let b = stream.read_u8().await?;
        if b == b'\n' || buf.len() > 4096 {
            break;
        }
        buf.push(b);
    }
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

fn device_path(adapter: &str, mac: &str) -> String {
    format!(
        "/org/bluez/{}/dev_{}",
        adapter,
        mac.replace(':', "_").to_uppercase()
    )
}

async fn device_call(
    bus: &Connection,
    adapter: &str,
    mac: &str,
    method: &str,
    uuid: Option<&str>,
) -> Result<(), String> {
    if mac.is_empty() {
        return Err(format!("{method} requires a MAC"));
    }
    let path = device_path(adapter, mac);
    let result = match uuid {
        Some(uuid) => {
            bus.call_method(
                Some("org.bluez"),
                path.as_str(),
                Some("org.bluez.Device1"),
                method,
                &(uuid,),
            )
            .await
        }
        None => {
            bus.call_method(
                Some("org.bluez"),
                path.as_str(),
                Some("org.bluez.Device1"),
                method,
                &(),
            )
            .await
        }
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

async fn remove_device(bus: &Connection, adapter: &str, mac: &str) -> Result<(), String> {
    if mac.is_empty() {
        return Err("remove requires a MAC".into());
    }
    let path = zbus::ObjectPath::try_from(device_path(adapter, mac)).map_err(|e| e.to_string())?;
    let adapter_path = format!("/org/bluez/{adapter}");
    bus.call_method(
        Some("org.bluez"),
        adapter_path.as_str(),
        Some("org.bluez.Adapter1"),
        "RemoveDevice",
        &(path,),
    )
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

async fn get_prop<T>(bus: &Connection, path: &str, prop: &str) -> Option<T>
where
    T: TryFrom<OwnedVariant>,
{
    let reply = bus
        .call_method(
            Some("org.bluez"),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.bluez.Device1", prop),
        )
        .await
        .ok()?;
    let value: OwnedVariant = reply.body().deserialize().ok()?;
    T::try_from(value).ok()
}

async fn list_paired(bus: &Connection, adapter: &str) -> Result<Vec<String>, String> {
    let reply = bus
        .call_method(
            Some("org.bluez"),
            "/",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await
        .map_err(|e| e.to_string())?;
    let objects: std::collections::HashMap<
        zbus::OwnedObjectPath,
        std::collections::HashMap<String, std::collections::HashMap<String, OwnedVariant>>,
    > = reply.body().deserialize().map_err(|e| e.to_string())?;

    let prefix = format!("/org/bluez/{adapter}/dev_");
    let mut devices: Vec<(bool, String, String, String)> = Vec::new();
    for (path, ifaces) in objects {
        let path = path.as_str().to_string();
        if !path.starts_with(&prefix) || !ifaces.contains_key("org.bluez.Device1") {
            continue;
        }
        if get_prop::<bool>(bus, &path, "Paired").await != Some(true) {
            continue;
        }
        let mac = get_prop::<String>(bus, &path, "Address")
            .await
            .unwrap_or_default()
            .to_uppercase();
        let name = match get_prop::<String>(bus, &path, "Name").await {
            Some(n) if !n.is_empty() => n,
            _ => get_prop::<String>(bus, &path, "Alias")
                .await
                .unwrap_or_default(),
        };
        let connected = get_prop::<bool>(bus, &path, "Connected")
            .await
            .unwrap_or(false);
        let trusted = get_prop::<bool>(bus, &path, "Trusted")
            .await
            .unwrap_or(false);
        let class = get_prop::<u32>(bus, &path, "Class").await.unwrap_or(0);
        devices.push((
            connected,
            name.clone(),
            mac.clone(),
            format!(
                "{{\"mac\":\"{mac}\",\"name\":\"{}\",\"connected\":{connected},\"trusted\":{trusted},\"class\":{class},\"path\":\"{path}\"}}",
                name.replace('"', "'")
            ),
        ));
    }

    devices.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
            .then_with(|| a.2.cmp(&b.2))
    });
    Ok(devices.into_iter().map(|d| d.3).collect())
}

async fn bluez_verb(bus: &Connection, verb: &str, arg: &str, adapter: &str) -> String {
    match verb {
        "list_paired" => match list_paired(bus, adapter).await {
            Ok(devices) => format!("{{\"ok\":true,\"devices\":[{}]}}", devices.join(",")),
            Err(e) => err_json(&e),
        },
        "connect" => {
            let (mac, uuid) = match arg.split_once(' ') {
                Some((m, u)) => (m, Some(u)),
                None => (arg, None),
            };
            action(
                device_call(
                    bus,
                    adapter,
                    mac,
                    "ConnectProfile",
                    uuid.or(Some(WAKE_UUID)),
                )
                .await,
            )
        }
        "disconnect-profile" => {
            let (mac, uuid) = match arg.split_once(' ') {
                Some((m, u)) => (m, Some(u)),
                None => (arg, None),
            };
            action(
                device_call(
                    bus,
                    adapter,
                    mac,
                    "DisconnectProfile",
                    uuid.or(Some(WAKE_UUID)),
                )
                .await,
            )
        }
        "connect-full" => action(device_call(bus, adapter, arg, "Connect", None).await),
        "disconnect" => action(device_call(bus, adapter, arg, "Disconnect", None).await),
        "remove" => action(remove_device(bus, adapter, arg).await),
        other => err_json(&format!("unknown command: {other}")),
    }
}
