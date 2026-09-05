use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use iap2_csm::messages::wifi::SecurityType;
use livi_runtime::bringup::CpConfig;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::livi_sock::{serve, Broadcaster, LiviSockConfig};
use livi_runtime::state::HelperState;
use livi_runtime::AsyncAuth;
use std::sync::Arc;

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
        adapter: "hci0".into(),
        identity: Identity { name: "LIVI".into(), ssid: "LIVI".into(), bt_mac: [0; 6] },
        cp: CpConfig {
            wifi_iface: "none0".into(),
            ssid: "LIVI".into(),
            passphrase: "12345678".into(),
            channel: 36,
            security_type: SecurityType::WpaWpa2,
            airplay_port: 7000,
            source_version: "950.7.1".into(),
            public_key: String::new(),
            transport: Transport::Wireless,
            av_iface: None,
            available_current_ma: 500,
        },
    }
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
    let server = tokio::spawn(serve(config(&path), MockAuth, Some(bus), bcast.clone(), state));

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
