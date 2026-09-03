// /tmp/aa-bt.sock: line-JSON RPC for paired-device management, plus an event stream.

use std::io;
use std::os::unix::fs::PermissionsExt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use zbus::zvariant::OwnedValue;
use zbus::Connection;

pub const SOCK_PATH: &str = "/tmp/aa-bt.sock";

// Default wake/profile target: the phone's HFP AG.
const WAKE_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";

type SetWiredPhones = Box<dyn Fn(Vec<String>) + Send + Sync>;
type SetPlaybackStatus = Box<dyn Fn(&str) + Send + Sync>;
type SetScoSink = Box<dyn Fn(Option<(String, u32)>) + Send + Sync>;

pub struct AaSockDeps {
    pub adapter: String,
    pub wifi_iface: String,
    /// Receives the phone ids LIVI reports as projecting over USB.
    pub set_wired_phones: SetWiredPhones,
    /// Long-lived event stream LIVI subscribes to.
    pub events: crate::livi_sock::Broadcaster,
    /// Mirrors the active session's play state into the AVRCP player.
    pub set_playback_status: SetPlaybackStatus,
    /// Where the call audio goes: the pipeline's feed path and stream id, or nothing.
    pub set_sco_sink: SetScoSink,
}

/// Without a D-Bus connection (macOS) the BlueZ verbs answer with an error, the rest works.
pub async fn serve(bus: Option<Connection>, deps: AaSockDeps) -> io::Result<()> {
    let _ = std::fs::remove_file(SOCK_PATH);
    let listener = UnixListener::bind(SOCK_PATH)?;
    std::fs::set_permissions(SOCK_PATH, std::fs::Permissions::from_mode(0o666))?;
    println!("[aa-sock] listening on {SOCK_PATH}");

    let deps = std::sync::Arc::new(deps);
    loop {
        let (stream, _) = listener.accept().await?;
        let bus = bus.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, bus, deps).await {
                eprintln!("[aa-sock] connection error: {e}");
            }
        });
    }
}

async fn handle(
    mut stream: UnixStream,
    bus: Option<Connection>,
    deps: std::sync::Arc<AaSockDeps>,
) -> io::Result<()> {
    let line = read_line(&mut stream).await?;
    let (verb, arg) = match line.split_once(' ') {
        Some((v, a)) => (v, a.trim()),
        None => (line.as_str(), ""),
    };

    // The subscription stays open for the lifetime of the connection; anything else is a
    // one-shot request/response.
    if verb == "subscribe" {
        return run_subscriber(stream, deps.events.clone()).await;
    }

    let json = match verb {
        "list_paired" | "connect" | "disconnect-profile" | "connect-full" | "disconnect" | "remove" => {
            match bus.as_ref() {
                None => err_json("bluetooth unavailable on this platform"),
                Some(bus) => bluez_verb(bus, verb, arg, &deps.adapter).await,
            }
        }
        "wired-phones" => {
            let ids: Vec<String> = serde_json::from_str(if arg.is_empty() { "[]" } else { arg })
                .unwrap_or_default();
            (deps.set_wired_phones)(ids);
            ok_json()
        }
        "sco-sink" => {
            let target = arg.split_once(' ').and_then(|(feed, id)| {
                id.trim().parse::<u32>().ok().map(|id| (feed.to_owned(), id))
            });
            (deps.set_sco_sink)(target);
            ok_json()
        }
        "playback-status" => match arg {
            "playing" | "paused" | "stopped" => {
                (deps.set_playback_status)(arg);
                ok_json()
            }
            other => err_json(&format!("unknown playback status: {other}")),
        },
        "deauth-ap" => {
            let count = deauth_ap(&deps.wifi_iface).await;
            println!("[aa-sock] deauth-ap: kicked {count} client(s)");
            format!("{{\"ok\":true,\"count\":{count}}}")
        }
        other => err_json(&format!("unknown command: {other}")),
    };

    stream.write_all(json.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await
}

/// Kicks every associated station off the access point via hostapd's control socket.
async fn deauth_ap(iface: &str) -> usize {
    let cli = |args: &[&str]| {
        tokio::process::Command::new("hostapd_cli")
            .args(["-p", "/var/run/hostapd", "-i", iface])
            .args(args)
            .output()
    };
    let Ok(out) = cli(&["list_sta"]).await else { return 0 };
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

/// Feeds pushed event lines to LIVI until it hangs up.
async fn run_subscriber(
    mut stream: UnixStream,
    events: crate::livi_sock::Broadcaster,
) -> io::Result<()> {
    println!("[aa-sock] event subscriber connected");
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
                    println!("[aa-sock] event subscriber gone");
                    return Ok(());
                }
            }
        }
    }
}

fn ok_json() -> String {
    "{\"ok\":true}".to_string()
}

fn err_json(msg: &str) -> String {
    let clean = msg.replace(['"', '\n', '\r'], " ");
    format!("{{\"ok\":false,\"error\":\"{}\"}}", clean.trim())
}

fn action(result: Result<(), String>) -> String {
    match result {
        Ok(()) => ok_json(),
        Err(e) => err_json(&e),
    }
}

async fn read_line(stream: &mut UnixStream) -> io::Result<String> {
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
    format!("/org/bluez/{}/dev_{}", adapter, mac.replace(':', "_").to_uppercase())
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
            bus.call_method(Some("org.bluez"), path.as_str(), Some("org.bluez.Device1"), method, &(uuid,))
                .await
        }
        None => {
            bus.call_method(Some("org.bluez"), path.as_str(), Some("org.bluez.Device1"), method, &())
                .await
        }
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

async fn remove_device(bus: &Connection, adapter: &str, mac: &str) -> Result<(), String> {
    if mac.is_empty() {
        return Err("remove requires a MAC".into());
    }
    let path = zbus::zvariant::ObjectPath::try_from(device_path(adapter, mac))
        .map_err(|e| e.to_string())?;
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
    T: TryFrom<OwnedValue>,
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
    let value: OwnedValue = reply.body().deserialize().ok()?;
    T::try_from(value).ok()
}

/// Paired phones as the UI lists them: connected first, then by name.
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
        zbus::zvariant::OwnedObjectPath,
        std::collections::HashMap<String, std::collections::HashMap<String, OwnedValue>>,
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
        let mac = get_prop::<String>(bus, &path, "Address").await.unwrap_or_default().to_uppercase();
        let name = match get_prop::<String>(bus, &path, "Name").await {
            Some(n) if !n.is_empty() => n,
            _ => get_prop::<String>(bus, &path, "Alias").await.unwrap_or_default(),
        };
        let connected = get_prop::<bool>(bus, &path, "Connected").await.unwrap_or(false);
        let trusted = get_prop::<bool>(bus, &path, "Trusted").await.unwrap_or(false);
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
            action(device_call(bus, adapter, mac, "ConnectProfile", uuid.or(Some(WAKE_UUID))).await)
        }
        "disconnect-profile" => {
            let (mac, uuid) = match arg.split_once(' ') {
                Some((m, u)) => (m, Some(u)),
                None => (arg, None),
            };
            action(device_call(bus, adapter, mac, "DisconnectProfile", uuid.or(Some(WAKE_UUID))).await)
        }
        "connect-full" => action(device_call(bus, adapter, arg, "Connect", None).await),
        "disconnect" => action(device_call(bus, adapter, arg, "Disconnect", None).await),
        "remove" => action(remove_device(bus, adapter, arg).await),
        other => err_json(&format!("unknown command: {other}")),
    }
}
