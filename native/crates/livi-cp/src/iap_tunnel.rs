// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::control_cipher::ControlCipher;
use crate::crypto::hkdf_sha512;
use crate::net;

const PKG_HEADER: usize = 32;
const MSG_TYPE_COMM: u32 = 0x636f_6d6d;
const MSG_TYPE_COMMAND: u32 = 0x636d_6e64;
const PACKAGE_ASYNC: u32 = 0x6173_796e;
const PACKAGE_SYNC: u32 = 0x7379_6e63;
const PACKAGE_REPLY: u32 = 0x7270_6c79;
const MAX_PACKAGE: usize = 4 * 1024 * 1024;

pub type Deliver = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

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
        let key = stream_key(shared, seed, b"DataStream-Output-Encryption-Key");
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

    pub fn listen_duplex(
        name: &'static str,
        shared: &[u8; 32],
        seed: &str,
        deliver: Deliver,
    ) -> io::Result<(Self, mpsc::UnboundedSender<Vec<u8>>)> {
        let keys = (
            stream_key(shared, seed, b"DataStream-Output-Encryption-Key"),
            stream_key(shared, seed, b"DataStream-Input-Encryption-Key"),
        );
        let (to_phone, outgoing) = mpsc::unbounded_channel();
        let outgoing = Arc::new(Mutex::new(outgoing));
        let listener = net::tcp_listener()?;
        let port = net::local_port(listener.local_addr());
        let task = tokio::spawn(async move {
            let mut current: Option<JoinHandle<()>> = None;
            while let Ok((sock, from)) = listener.accept().await {
                println!("[cpIapTunnel] {name} data connection from {from}");
                if let Some(old) = current.take() {
                    old.abort();
                }
                let (deliver, outgoing) = (deliver.clone(), outgoing.clone());
                current = Some(tokio::spawn(async move {
                    let mut outgoing = outgoing.lock().await;
                    duplex(name, sock, keys, &deliver, &mut outgoing).await;
                }));
            }
        });
        Ok((Self { port, task }, to_phone))
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

fn stream_key(shared: &[u8; 32], seed: &str, info: &[u8]) -> [u8; 32] {
    hkdf_sha512(shared, format!("DataStream-Salt{seed}").as_bytes(), info)
}

struct Package {
    kind: u32,
    message_type: u32,
    reply_token: u64,
    body: Vec<u8>,
}

fn next_package(plain: &mut Vec<u8>) -> Result<Option<Package>, usize> {
    if plain.len() < PKG_HEADER {
        return Ok(None);
    }
    let size = u32::from_be_bytes(plain[..4].try_into().expect("4 bytes")) as usize;
    if !(PKG_HEADER..=MAX_PACKAGE).contains(&size) {
        return Err(size);
    }
    if plain.len() < size {
        return Ok(None);
    }
    let package = Package {
        kind: u32::from_be_bytes(plain[4..8].try_into().expect("4 bytes")),
        message_type: u32::from_be_bytes(plain[16..20].try_into().expect("4 bytes")),
        reply_token: u64::from_be_bytes(plain[20..28].try_into().expect("8 bytes")),
        body: plain[PKG_HEADER..size].to_vec(),
    };
    plain.drain(..size);
    Ok(Some(package))
}

fn package(kind: u32, message_type: u32, reply_token: u64, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(PKG_HEADER + body.len());
    p.extend_from_slice(&((PKG_HEADER + body.len()) as u32).to_be_bytes());
    p.extend_from_slice(&kind.to_be_bytes());
    p.extend_from_slice(&0u64.to_be_bytes());
    p.extend_from_slice(&message_type.to_be_bytes());
    p.extend_from_slice(&reply_token.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(body);
    p
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
        loop {
            match next_package(&mut plain) {
                Ok(Some(p)) if p.message_type == MSG_TYPE_COMM => {
                    let _ = out.send(p.body);
                }
                Ok(Some(p)) => {
                    if name != "iAP" {
                        let tag =
                            String::from_utf8_lossy(&p.message_type.to_be_bytes()).into_owned();
                        println!(
                            "[cpIapTunnel] {name}: package '{tag}' ({} B) skipped",
                            p.body.len()
                        );
                    }
                }
                Ok(None) => break,
                Err(size) => {
                    println!("[cpIapTunnel] implausible package size {size}, dropping");
                    return;
                }
            }
        }
    }
    println!("[cpIapTunnel] {name} data connection closed");
}

async fn duplex(
    name: &str,
    sock: TcpStream,
    (read_key, write_key): ([u8; 32], [u8; 32]),
    deliver: &Deliver,
    outgoing: &mut mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let (mut rd, mut wr) = sock.into_split();
    let mut cipher = ControlCipher::new(read_key, write_key);
    let mut sealed = Vec::new();
    let mut plain = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = rd.read(&mut buf) => {
                let n = match read {
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
                        println!("[cpIapTunnel] {name} stream decrypt failed");
                        return;
                    }
                }
                loop {
                    match next_package(&mut plain) {
                        Ok(Some(p)) if p.kind == PACKAGE_REPLY => {}
                        Ok(Some(p)) => {
                            if p.kind == PACKAGE_SYNC {
                                let ack = package(PACKAGE_REPLY, 0, p.reply_token, &[]);
                                if wr.write_all(&cipher.encrypt(&ack)).await.is_err() {
                                    return;
                                }
                            }
                            deliver(p.body);
                        }
                        Ok(None) => break,
                        Err(size) => {
                            println!("[cpIapTunnel] {name}: implausible package size {size}, dropping");
                            return;
                        }
                    }
                }
            }
            message = outgoing.recv() => {
                let Some(message) = message else { break };
                let p = package(PACKAGE_ASYNC, MSG_TYPE_COMMAND, 0, &message);
                if wr.write_all(&cipher.encrypt(&p)).await.is_err() {
                    break;
                }
            }
        }
    }
    println!("[cpIapTunnel] {name} data connection closed");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn comm(message_type: u32, body: &[u8]) -> Vec<u8> {
        package(PACKAGE_ASYNC, message_type, 0, body)
    }

    #[tokio::test]
    async fn comm_packages_carry_iap2_out() {
        let shared = [7u8; 32];
        let (tx, mut rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&shared, "42", tx).unwrap();
        let key = hkdf_sha512(
            &shared,
            b"DataStream-Salt42",
            b"DataStream-Output-Encryption-Key",
        );
        let mut phone_side = ControlCipher::new([0; 32], key);

        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port()))
            .await
            .unwrap();
        let stream = [
            comm(MSG_TYPE_COMM, b"iap2"),
            comm(1, b"other"),
            comm(MSG_TYPE_COMM, b"more"),
        ]
        .concat();
        let sealed = phone_side.encrypt(&stream);
        sock.write_all(&sealed[..5]).await.unwrap();
        sock.write_all(&sealed[5..]).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), b"iap2");
        assert_eq!(rx.recv().await.unwrap(), b"more");
    }

    async fn packages(
        sock: &mut TcpStream,
        cipher: &mut ControlCipher,
        want: usize,
    ) -> Vec<Package> {
        let (mut sealed, mut plain, mut got) = (Vec::new(), Vec::new(), Vec::new());
        let mut buf = vec![0u8; 4096];
        while got.len() < want {
            let read = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf));
            let n = read.await.unwrap().unwrap();
            assert!(n > 0, "connection closed");
            sealed.extend_from_slice(&buf[..n]);
            let (data, rest) = cipher.decrypt(&sealed).unwrap();
            plain.extend(data);
            sealed = rest;
            while let Ok(Some(p)) = next_package(&mut plain) {
                got.push(p);
            }
        }
        got
    }

    #[tokio::test]
    async fn a_duplex_stream_acknowledges_sync_and_answers_as_a_command() {
        let shared = [5u8; 32];
        let (tx, mut rx) = mpsc::unbounded_channel();
        let deliver: Deliver = Arc::new(move |m| {
            let _ = tx.send(m);
        });
        let (tunnel, to_phone) = IapTunnel::listen_duplex("test", &shared, "7", deliver).unwrap();
        let key = |info: &[u8]| hkdf_sha512(&shared, b"DataStream-Salt7", info);
        let phone_cipher = || {
            ControlCipher::new(
                key(b"DataStream-Input-Encryption-Key"),
                key(b"DataStream-Output-Encryption-Key"),
            )
        };

        let mut phone = phone_cipher();
        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port()))
            .await
            .unwrap();
        let stream = [
            package(PACKAGE_SYNC, MSG_TYPE_COMMAND, 0x1122, b"ask"),
            package(PACKAGE_REPLY, 0, 9, b"an answer to the car"),
            package(PACKAGE_ASYNC, 0x1234, 0, b"tell"),
        ]
        .concat();
        sock.write_all(&phone.encrypt(&stream)).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), b"ask");
        assert_eq!(rx.recv().await.unwrap(), b"tell");
        to_phone.send(b"answer".to_vec()).unwrap();
        let got = packages(&mut sock, &mut phone, 2).await;
        assert_eq!((got[0].kind, got[0].reply_token), (PACKAGE_REPLY, 0x1122));
        assert!(got[0].body.is_empty());
        assert_eq!(
            (got[1].kind, got[1].message_type),
            (PACKAGE_ASYNC, MSG_TYPE_COMMAND)
        );
        assert_eq!(got[1].body, b"answer");

        let mut phone = phone_cipher();
        let mut again = TcpStream::connect(("127.0.0.1", tunnel.port()))
            .await
            .unwrap();
        let mut buf = [0u8; 64];
        while sock.read(&mut buf).await.unwrap_or(0) > 0 {}
        to_phone.send(b"later".to_vec()).unwrap();
        assert_eq!(packages(&mut again, &mut phone, 1).await[0].body, b"later");
    }

    #[tokio::test]
    async fn garbage_ends_the_connection() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&[1; 32], "1", tx).unwrap();
        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port()))
            .await
            .unwrap();
        sock.write_all(&[
            4, 0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ])
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
        let key = hkdf_sha512(
            &shared,
            b"DataStream-Salt9",
            b"DataStream-Output-Encryption-Key",
        );
        let mut phone_side = ControlCipher::new([0; 32], key);
        let mut sock = TcpStream::connect(("127.0.0.1", tunnel.port()))
            .await
            .unwrap();
        sock.write_all(&phone_side.encrypt(&[0u8; PKG_HEADER]))
            .await
            .unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(sock.read(&mut buf).await.unwrap_or(0), 0);
    }
}
