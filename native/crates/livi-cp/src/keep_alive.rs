// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;

use tokio::task::JoinHandle;

use crate::net;

pub struct KeepAlive {
    port: u16,
    task: JoinHandle<()>,
}

impl KeepAlive {
    pub fn listen() -> io::Result<Self> {
        let sock = net::udp_socket()?;
        let port = net::local_port(sock.local_addr());
        let task = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            while sock.recv_from(&mut buf).await.is_ok() {}
        });
        Ok(Self { port, task })
    }

    pub fn port(&self) -> u16 {
        self.port
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
    async fn it_takes_datagrams() {
        let alive = KeepAlive::listen().unwrap();
        let phone = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        phone
            .send_to(b"ka", ("127.0.0.1", alive.port()))
            .await
            .unwrap();
        assert!(alive.port() > 0);
    }
}
