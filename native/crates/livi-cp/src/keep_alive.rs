// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::net;

pub struct KeepAlive {
    port: u16,
    heard: Arc<Mutex<Option<Instant>>>,
    task: JoinHandle<()>,
}

impl KeepAlive {
    pub fn listen() -> io::Result<Self> {
        let sock = net::udp_socket()?;
        let port = net::local_port(sock.local_addr());
        let heard = Arc::new(Mutex::new(None));
        let task = tokio::spawn(beacons(sock, heard.clone()));
        Ok(Self { port, heard, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn last_heard(&self) -> Option<Instant> {
        *self.heard.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn beacons(sock: tokio::net::UdpSocket, heard: Arc<Mutex<Option<Instant>>>) {
    let mut buf = [0u8; 512];
    while sock.recv_from(&mut buf).await.is_ok() {
        *heard.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_beacon_is_heard() {
        let alive = KeepAlive::listen().unwrap();
        let phone = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        phone
            .send_to(b"ka", ("127.0.0.1", alive.port()))
            .await
            .unwrap();
        assert!(alive.port() > 0);
        for _ in 0..100 {
            if alive.last_heard().is_some() {
                return;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the beacon was not heard");
    }
}
