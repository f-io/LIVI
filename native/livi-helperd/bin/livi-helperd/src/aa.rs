// Android Auto wireless bootstrap over the AA RFCOMM channel.

use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use livi_aa::wpp;
use livi_runtime::bt::IncomingConn;
use livi_runtime::livi_sock::Broadcaster;
use livi_runtime::net;

const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AaConfig {
    pub ssid: String,
    pub passphrase: String,
    pub channel: u16,
    pub wifi_iface: String,
    pub ap_ip: String,
    pub port: u16,
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
        ids.iter()
            .any(|id| !id.is_empty() && known.contains(&id.to_uppercase()))
    }
}

pub async fn watch(
    mut incoming: tokio::sync::mpsc::UnboundedReceiver<IncomingConn>,
    cfg: AaConfig,
    bcast: Broadcaster,
    wired: WiredPhones,
    hfp: livi_runtime::hfp::Hfp,
) {
    while let Some(conn) = incoming.recv().await {
        println!("[aa] phone connected mac={}", conn.peer_mac);
        hfp.trigger(&conn.peer_mac);
        let (cfg, bcast, wired) = (cfg.clone(), bcast.clone(), wired.clone());
        tokio::spawn(async move {
            if let Err(e) = handshake(conn.fd, &conn.peer_mac, &cfg, &bcast, &wired).await {
                println!("[aa] {}: bootstrap ended: {e}", conn.peer_mac);
            }
        });
    }
}

async fn handshake(
    fd: OwnedFd,
    mac: &str,
    cfg: &AaConfig,
    bcast: &Broadcaster,
    wired: &WiredPhones,
) -> std::io::Result<()> {
    let std_stream = std::os::unix::net::UnixStream::from(fd);
    std_stream.set_nonblocking(true)?;
    let mut sock = UnixStream::from_std(std_stream)?;

    let bssid = net::wlan_mac(&cfg.wifi_iface)
        .unwrap_or_default()
        .to_lowercase();
    println!(
        "[aa] {mac}: WPP bootstrap (AP {}:{} ssid={})",
        cfg.ap_ip, cfg.port, cfg.ssid
    );

    sock.write_all(&wpp::wifi_version_request(cfg.channel))
        .await?;

    let mut reader = wpp::FrameReader::default();
    let mut buf = [0u8; 4096];
    let mut pending: Option<(u16, Vec<u8>)> = None;

    // The phone answers the version request first; a wired phone is dropped here so the
    // USB session keeps the phone.
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

    sock.write_all(&wpp::wifi_start_request(&cfg.ap_ip, cfg.port))
        .await?;

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
                sock.write_all(&wpp::wifi_info_response(&cfg.ssid, &cfg.passphrase, &bssid))
                    .await?;
            }
            wpp::MSG_WIFI_CONNECTION_STATUS => {
                let status = body.get(1).copied().unwrap_or(0);
                if status == 0 {
                    println!("[aa] {mac}: phone joined AP {}", cfg.ssid);
                } else {
                    println!("[aa] {mac}: phone-side AP join failed (status={status})");
                }
            }
            wpp::MSG_PING => sock.write_all(&wpp::pong(&body)).await?,
            wpp::MSG_WIFI_VERSION_RESPONSE => {
                let id = wpp::parse_identity(&body);
                emit_device(bcast, mac, &id);
            }
            wpp::MSG_WIFI_START_RESPONSE => {}
            other => println!(
                "[aa] {mac}: unknown WPP message {other} ({} bytes)",
                body.len()
            ),
        }
    }
}

async fn read_frame(
    sock: &mut UnixStream,
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
        if id.instance_id.is_empty() {
            "-"
        } else {
            &id.instance_id
        },
        if id.serial.is_empty() {
            "-"
        } else {
            &id.serial
        }
    );
    bcast.push_json(format!(
        "{{\"event\":\"aa-device\",\"btMac\":\"{}\",\"instanceId\":\"{}\",\"usbSerial\":\"{}\"}}",
        mac.to_uppercase(),
        id.instance_id,
        id.serial
    ));
}
