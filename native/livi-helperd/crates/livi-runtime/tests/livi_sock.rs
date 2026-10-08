use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use livi_runtime::AsyncAuth;
use livi_runtime::bringup::CpConfig;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{Bluez, Broadcaster, LiviSockConfig, serve};
use livi_runtime::state::HelperState;
use livi_runtime::vehicle::Fuels;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

#[derive(Clone)]
struct MockAuth;

impl AsyncAuth for MockAuth {
    async fn read_certificate(&mut self) -> Result<Vec<u8>, String> {
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF])
    }
    async fn sign(&mut self, challenge: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(challenge.iter().rev().copied().collect())
    }
    async fn protocol_major(&mut self) -> Result<u8, String> {
        Ok(3)
    }
}

fn config(path: &str) -> LiviSockConfig {
    LiviSockConfig {
        path: path.into(),
        identity: Identity {
            name: "LIVI".into(),
            ssid: "LIVI".into(),
            bt_mac: [0; 6],
            fuels: Fuels::default(),
        },
        cp: CpConfig {
            ap_mac: None,
            ap_on_air: None,
            wifi_iface: "none0".into(),
            ssid: "LIVI".into(),
            passphrase: "12345678".into(),
            channel: livi_wifi::Channel::of_number(36),
            airplay_port: 7000,
            source_version: "950.7.1".into(),
            public_key: String::new(),
            transport: Transport::Wireless,
            av_iface: None,
            av_iface_late: None,
            available_current_ma: 500,
            on_cable: None,
            start_again: None,
        },
        disconnect: None,
        targets: None,
        cp_live: None,
    }
}

fn no_bluez() -> watch::Receiver<Option<Bluez>> {
    watch::channel(None).1
}

async fn listening(path: &str) {
    for _ in 0..50 {
        if UnixStream::connect(path).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_wired_phone_hears_the_start_again_without_its_session_ending() {
    let path = std::env::temp_dir()
        .join(format!("livi-sock-cable-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let state = Arc::new(HelperState::default());
    let again = Arc::new(Notify::new());
    state.wired_started("00008120", Arc::new(Notify::new()), again.clone());
    let server =
        tokio::spawn(serve(config(&path), MockAuth, no_bluez(), Broadcaster::default(), state));
    listening(&path).await;

    assert!(request(&path, "start-wired 00008120").await.contains("\"ok\":true"));
    again.notified().await;
    assert!(request(&path, "start-wired 00008030").await.contains("\"ok\":false"));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn before_bluetooth_is_up_a_phone_cannot_be_dropped_but_the_socket_serves() {
    let path = std::env::temp_dir()
        .join(format!("livi-sock-no-bt-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let state = Arc::new(HelperState::default());
    let (_bluez, later) = watch::channel(None);
    let server = tokio::spawn(serve(config(&path), MockAuth, later, Broadcaster::default(), state));
    listening(&path).await;

    let dropped = request(&path, "disconnect 0C:6A:C4:4E:F3:2A").await;
    assert!(dropped.contains("\"ok\":false"), "{dropped}");
    assert!(request(&path, "certificate").await.contains("\"ok\":true"));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

async fn request(path: &str, line: &str) -> String {
    let stream = UnixStream::connect(path).await.unwrap();
    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(format!("{line}\n").as_bytes()).await.unwrap();
    let mut resp = String::new();
    reader.read_line(&mut resp).await.unwrap();
    resp.trim().to_string()
}

#[tokio::test]
async fn certificate_sign_and_subscribe() {
    let dir = std::env::temp_dir().join(format!("livi-sock-{}", std::process::id()));
    let path = dir.to_string_lossy().to_string();
    let bus = match zbus::Connection::system().await {
        Ok(b) => b,
        Err(_) => return, // no system bus in CI; RPC paths not exercising bluez still tested elsewhere
    };
    let bcast = Broadcaster::default();
    let state = Arc::new(HelperState::default());
    let bluez = watch::channel(Some(Bluez { bus, adapter: "hci0".into(), bt_mac: [0; 6] })).1;
    let server = tokio::spawn(serve(config(&path), MockAuth, bluez, bcast.clone(), state));

    for _ in 0..50 {
        if UnixStream::connect(&path).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let cert = request(&path, "certificate").await;
    assert!(cert.contains("\"ok\":true"));
    assert!(cert.contains(&STANDARD.encode([0xDE, 0xAD, 0xBE, 0xEF])));
    assert!(cert.contains("\"protocolMajor\":3"));

    let digest = STANDARD.encode([1u8, 2, 3, 4]);
    let sig = request(&path, &format!("sign {digest}")).await;
    assert!(sig.contains(&STANDARD.encode([4u8, 3, 2, 1])));

    let unknown = request(&path, "bogus").await;
    assert!(unknown.contains("\"ok\":false"));

    // subscribe receives pushed lines
    let stream = UnixStream::connect(&path).await.unwrap();
    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(b"subscribe\n").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    bcast.push_json("{\"type\":\"nowplaying\",\"title\":\"Song\"}".into());
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(line.contains("nowplaying"));

    server.abort();
    let _ = std::fs::remove_file(&path);
}
