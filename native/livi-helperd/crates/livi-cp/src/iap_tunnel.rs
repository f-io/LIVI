//! iAP2 over CarPlay (stream type 130), used once the phone sent disableBluetooth.
//! Inside the control channel framing, keyed from the SETUP seed, 32-byte big-endian
//! package headers carry raw iAP2 in their 'comm' packages. This side only reads,
//! the way back to the phone is the event channel.

use std::io;

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::control_cipher::ControlCipher;
use crate::crypto::hkdf_sha512;
use crate::net;

const PKG_HEADER: usize = 32;
const MSG_TYPE_COMM: u32 = 0x636f_6d6d;
const MAX_PACKAGE: usize = 4 * 1024 * 1024;

pub struct IapTunnel {
    port: u16,
    task: JoinHandle<()>,
}

impl IapTunnel {
    pub fn listen(
        shared: &[u8; 32],
        seed: &str,
        out: mpsc::UnboundedSender<Vec<u8>>,
    ) -> io::Result<Self> {
        Self::listen_as("iAP", shared, seed, out)
    }

    pub fn listen_as(
        name: &'static str,
        shared: &[u8; 32],
        seed: &str,
        out: mpsc::UnboundedSender<Vec<u8>>,
    ) -> io::Result<Self> {
        let key = hkdf_sha512(
            shared,
            format!("DataStream-Salt{seed}").as_bytes(),
            b"DataStream-Output-Encryption-Key",
        );
        let listener = net::tcp_listener()?;
        let port = net::local_port(listener.local_addr());
        let task = tokio::spawn(async move {
            let mut current: Option<JoinHandle<()>> = None;
            while let Ok((sock, from)) = listener.accept().await {
                println!("[cpIapTunnel] {name} data connection from {from}");
                if let Some(old) = current.take() {
                    old.abort();
                }
                current = Some(tokio::spawn(read(name, sock, key, out.clone())));
            }
        });
        Ok(Self { port, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for IapTunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read(name: &str, mut sock: TcpStream, key: [u8; 32], out: mpsc::UnboundedSender<Vec<u8>>) {
    let mut cipher = ControlCipher::new(key, [0; 32]);
    let mut sealed = Vec::new();
    let mut plain = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match sock.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        sealed.extend_from_slice(&buf[..n]);
        match cipher.decrypt(&sealed) {
            Ok((data, rest)) => {
                plain.extend(data);
                sealed = rest;
            }
            Err(_) => {
                println!("[cpIapTunnel] stream decrypt failed");
                return;
            }
        }
        while plain.len() >= PKG_HEADER {
            let size = u32::from_be_bytes(plain[..4].try_into().expect("4 bytes")) as usize;
            if !(PKG_HEADER..=MAX_PACKAGE).contains(&size) {
                println!("[cpIapTunnel] implausible package size {size}, dropping");
                return;
            }
            if plain.len() < size {
                break;
            }
            let kind = u32::from_be_bytes(plain[16..20].try_into().expect("4 bytes"));
            let body = plain[PKG_HEADER..size].to_vec();
            plain.drain(..size);
            if kind == MSG_TYPE_COMM {
                let _ = out.send(body);
            } else if name != "iAP" {
                let tag = String::from_utf8_lossy(&kind.to_be_bytes()).into_owned();
                println!("[cpIapTunnel] {name}: package '{tag}' ({} B) skipped", body.len());
            }
        }
    }
    println!("[cpIapTunnel] {name} data connection closed");
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    fn package(kind: u32, body: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; PKG_HEADER];
        p[..4].copy_from_slice(&((PKG_HEADER + body.len()) as u32).to_be_bytes());
        p[16..20].copy_from_slice(&kind.to_be_bytes());
        p.extend_from_slice(body);
        p
    }

    #[tokio::test]
    async fn comm_packages_carry_iap2_out() {
        let shared = [7u8; 32];
        let (tx, mut rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&shared, "42", tx).unwrap();
        let key = hkdf_sha512(&shared, b"DataStream-Salt42", b"DataStream-Output-Encryption-Key");
        let mut phone_side = ControlCipher::new([0; 32], key);

        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port())).await.unwrap();
        let stream = [
            package(MSG_TYPE_COMM, b"iap2"),
            package(1, b"other"),
            package(MSG_TYPE_COMM, b"more"),
        ]
        .concat();
        let sealed = phone_side.encrypt(&stream);
        sock.write_all(&sealed[..5]).await.unwrap();
        sock.write_all(&sealed[5..]).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), b"iap2");
        assert_eq!(rx.recv().await.unwrap(), b"more");
    }

    #[tokio::test]
    async fn garbage_ends_the_connection() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&[1; 32], "1", tx).unwrap();
        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port())).await.unwrap();
        sock.write_all(&[4, 0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(sock.read(&mut buf).await.unwrap_or(0), 0);
    }

    #[tokio::test]
    async fn an_implausible_package_ends_the_connection() {
        let shared = [3u8; 32];
        let (tx, _rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&shared, "9", tx).unwrap();
        let key = hkdf_sha512(&shared, b"DataStream-Salt9", b"DataStream-Output-Encryption-Key");
        let mut phone_side = ControlCipher::new([0; 32], key);
        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port())).await.unwrap();
        sock.write_all(&phone_side.encrypt(&[0u8; PKG_HEADER])).await.unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(sock.read(&mut buf).await.unwrap_or(0), 0);
    }
}
