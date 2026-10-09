use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use livi_aa::wpp;
use livi_runtime::livi_sock::Broadcaster;
use livi_runtime::net;
use livi_wifi::{Channel, Security};

const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "macos")]
const AP_WAIT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct AaConfig {
    pub ssid: String,
    pub passphrase: String,
    pub channel: Channel,
    #[cfg(target_os = "linux")]
    pub wifi_iface: String,
    pub ap_ip: String,
    pub port: u16,
}

/// The last one is the BSSID.
type Told = (AaConfig, Security, String);

/// What the phone is told: the access point on air, else the configured one.
#[cfg(target_os = "linux")]
async fn access_point(cfg: &AaConfig) -> Told {
    let base = cfg.clone();
    tokio::task::spawn_blocking(move || {
        let dc = crate::linux_main::DeviceConfig::load();
        let iface = dc.string("wifiInterface", "LIVI_WIFI_IFACE", &base.wifi_iface);
        let passphrase = dc.string("wifiPassword", "LIVI_PASSPHRASE", &base.passphrase);
        if iface == livi_link_host::link::CHOICE {
            return on_dongle(&base, passphrase);
        }
        let live = livi_link_host::on_air(&iface);
        let ssid = live.as_ref().map(|ap| ap.ssid.clone()).filter(|s| !s.is_empty());
        let channel = live.as_ref().map_or(base.channel, |ap| ap.channel);
        let security = live.map(|ap| ap.security).unwrap_or_default();
        let bssid = net::wlan_mac(&iface).unwrap_or_default();
        let told = AaConfig {
            ssid: ssid.unwrap_or_else(|| base.ssid.clone()),
            channel,
            wifi_iface: iface,
            passphrase,
            ..base
        };
        (told, security, bssid)
    })
    .await
    .unwrap_or_else(|_| (cfg.clone(), Security::default(), String::new()))
}

fn on_dongle(base: &AaConfig, passphrase: String) -> Told {
    use livi_link_host::{ap, link};
    let live = livi_link_host::on_air(link::CHOICE);
    let told = AaConfig {
        ssid: live
            .as_ref()
            .map(|ap| ap.ssid.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base.ssid.clone()),
        channel: live.as_ref().map_or(base.channel, |ap| ap.channel),
        ap_ip: ip_facing_dongle().unwrap_or_else(|| base.ap_ip.clone()),
        passphrase,
        ..base.clone()
    };
    (told, live.map(|ap| ap.security).unwrap_or_default(), ap::mac().unwrap_or_default())
}

fn ip_facing_dongle() -> Option<String> {
    #[cfg(target_os = "linux")]
    let iface = livi_link_host::link::host_iface();
    #[cfg(not(target_os = "linux"))]
    let iface = net::iface_facing(livi_link_host::link::LINK_NAME);
    iface.and_then(|iface| net::ipv4_of(&iface)).map(|a| a.to_string())
}

/// Phones already projecting over USB, which must not be invited onto the access point.
#[derive(Clone, Default)]
pub struct WiredPhones(Arc<Mutex<Vec<String>>>);

impl WiredPhones {
    pub fn set(&self, ids: Vec<String>) {
        *self.0.lock().unwrap() = ids.into_iter().map(|s| s.to_uppercase()).collect();
    }

    fn contains(&self, ids: &[&str]) -> bool {
        let known = self.0.lock().unwrap();
        ids.iter().any(|id| !id.is_empty() && known.contains(&id.to_uppercase()))
    }
}

#[cfg(target_os = "linux")]
pub async fn watch(
    mut incoming: tokio::sync::mpsc::UnboundedReceiver<livi_runtime::bt::IncomingConn>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    state: Arc<livi_runtime::state::HelperState>,
) {
    while let Some(conn) = incoming.recv().await {
        println!("[aa] phone connected mac={}", conn.peer_mac);
        let (cfg, bcast, wired, state) = (cfg.clone(), bcast.clone(), wired.clone(), state.clone());
        tokio::spawn(async move {
            state.link_up(&conn.peer_mac);
            let ended = over_bluez(conn.fd, &conn.peer_mac, &cfg, &bcast, &wired).await;
            state.link_down(&conn.peer_mac);
            if let Err(e) = ended {
                println!("[aa] {}: bootstrap ended: {e}", conn.peer_mac);
            }
        });
    }
}

#[cfg(target_os = "linux")]
async fn over_bluez(
    fd: std::os::fd::OwnedFd,
    mac: &str,
    cfg: &AaConfig,
    bcast: &Broadcaster,
    wired: &WiredPhones,
) -> std::io::Result<()> {
    let std_stream = std::os::unix::net::UnixStream::from(fd);
    std_stream.set_nonblocking(true)?;
    let sock = tokio::net::UnixStream::from_std(std_stream)?;
    handshake(sock, mac, access_point(cfg).await, bcast, wired).await
}

#[cfg(target_os = "macos")]
pub async fn watch_dongle(
    mut arrivals: tokio::sync::mpsc::UnboundedReceiver<crate::bt_dongle::Arrival>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    state: Arc<livi_runtime::state::HelperState>,
) {
    while let Some(arrival) = arrivals.recv().await {
        let (cfg, bcast, wired, state) = (cfg.clone(), bcast.clone(), wired.clone(), state.clone());
        tokio::spawn(async move {
            let mac = arrival.peer;
            println!("[aa] phone connected mac={mac} over the dongle");
            let up = tokio::task::spawn_blocking(|| livi_link_host::ap::ready(AP_WAIT)).await;
            if !up.unwrap_or(false) {
                eprintln!("[aa] {mac}: the dongle's access point is not up");
                return;
            }
            let told =
                tokio::task::spawn_blocking(move || on_dongle(&cfg, cfg.passphrase.clone())).await;
            let Ok(told) = told else { return };
            state.link_up(&mac);
            let ended = handshake(arrival.stream, &mac, told, &bcast, &wired).await;
            state.link_down(&mac);
            if let Err(e) = ended {
                println!("[aa] {mac}: bootstrap ended: {e}");
            }
        });
    }
}

async fn handshake(
    mut sock: impl AsyncRead + AsyncWrite + Unpin,
    mac: &str,
    told: Told,
    bcast: &Broadcaster,
    wired: &WiredPhones,
) -> std::io::Result<()> {
    let (cfg, security, bssid) = told;
    let (cfg, bssid) = (&cfg, bssid.to_lowercase());
    println!("[aa] {mac}: WPP bootstrap (AP {}:{} ssid={})", cfg.ap_ip, cfg.port, cfg.ssid);

    sock.write_all(&wpp::wifi_version_request(cfg.channel)).await?;

    let mut reader = wpp::FrameReader::default();
    let mut buf = [0u8; 4096];
    let mut pending: Option<(u16, Vec<u8>)> = None;

    // The phone answers the version request first. A wired phone is dropped here so its USB
    // session keeps it.
    if let Some((msg_id, body)) =
        read_frame(&mut sock, &mut reader, &mut buf, VERSION_TIMEOUT).await?
    {
        if msg_id == wpp::MSG_WIFI_VERSION_RESPONSE {
            let id = wpp::parse_identity(&body);
            emit_device(bcast, mac, &id);
            if wired.contains(&[mac, &id.instance_id, &id.serial]) {
                println!("[aa] {mac}: already projecting over USB, not offering the AP");
                return Ok(());
            }
        } else {
            pending = Some((msg_id, body));
        }
    }

    sock.write_all(&wpp::wifi_start_request(&cfg.ap_ip, cfg.port)).await?;

    loop {
        let next = match pending.take() {
            Some(f) => Some(f),
            None => read_frame(&mut sock, &mut reader, &mut buf, HANDSHAKE_TIMEOUT).await?,
        };
        let Some((msg_id, body)) = next else {
            println!("[aa] {mac}: WPP handshake timed out");
            return Ok(());
        };

        match msg_id {
            wpp::MSG_WIFI_INFO_REQUEST => {
                let info = wpp::wifi_info_response(&cfg.ssid, &cfg.passphrase, &bssid, security);
                sock.write_all(&info).await?;
            }
            wpp::MSG_WIFI_CONNECT_STATUS => {
                let status = wpp::connect_status(&body);
                if status == 0 {
                    println!("[aa] {mac}: phone joined AP {}", cfg.ssid);
                } else {
                    println!("[aa] {mac}: phone-side AP join failed (status={status})");
                }
            }
            wpp::MSG_WIFI_PING_REQUEST => sock.write_all(&wpp::pong(&body)).await?,
            wpp::MSG_WIFI_VERSION_RESPONSE => {
                let id = wpp::parse_identity(&body);
                emit_device(bcast, mac, &id);
            }
            wpp::MSG_WIFI_START_RESPONSE => {}
            other => println!("[aa] {mac}: unknown WPP message {other} ({} bytes)", body.len()),
        }
    }
}

async fn read_frame(
    sock: &mut (impl AsyncRead + Unpin),
    reader: &mut wpp::FrameReader,
    buf: &mut [u8],
    timeout: Duration,
) -> std::io::Result<Option<(u16, Vec<u8>)>> {
    loop {
        if let Some(frame) = reader.next_frame() {
            return Ok(Some(frame));
        }
        let n = match tokio::time::timeout(timeout, sock.read(buf)).await {
            Err(_) => return Ok(None),
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
        };
        reader.push(&buf[..n]);
    }
}

fn emit_device(bcast: &Broadcaster, mac: &str, id: &wpp::Identity) {
    if id.instance_id.is_empty() && id.serial.is_empty() {
        return;
    }
    println!(
        "[aa] {mac}: identified instanceId={} serial={}",
        if id.instance_id.is_empty() { "-" } else { &id.instance_id },
        if id.serial.is_empty() { "-" } else { &id.serial }
    );
    bcast.push_json(format!(
        "{{\"event\":\"aa-device\",\"btMac\":\"{}\",\"instanceId\":\"{}\",\"usbSerial\":\"{}\"}}",
        mac.to_uppercase(),
        id.instance_id,
        id.serial
    ));
}
