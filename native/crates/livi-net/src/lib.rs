// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub mod bridge;
pub mod port;

pub fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> io::Result<TcpStream> {
    let mut last = None;
    for candidate in ipv4_first(addr.to_socket_addrs()?) {
        match TcpStream::connect_timeout(&candidate, timeout) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "resolves to no address")))
}

fn ipv4_first(addrs: impl Iterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut addrs: Vec<SocketAddr> = addrs.collect();
    addrs.sort_by_key(SocketAddr::is_ipv6);
    addrs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const TIMEOUT: Duration = Duration::from_secs(2);

    #[test]
    fn ipv4_comes_first_and_each_family_keeps_its_order() {
        let addrs: Vec<SocketAddr> = [
            "[fe80::1]:5000",
            "10.10.10.1:5000",
            "[::1]:5000",
            "127.0.0.1:5000",
        ]
        .iter()
        .map(|a| a.parse().unwrap())
        .collect();
        let sorted: Vec<String> = ipv4_first(addrs.into_iter())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            sorted,
            [
                "10.10.10.1:5000",
                "127.0.0.1:5000",
                "[fe80::1]:5000",
                "[::1]:5000"
            ]
        );
    }

    #[test]
    fn an_address_that_does_not_answer_gives_way_to_the_next() {
        let open = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let addrs = [closed, open.local_addr().unwrap()];
        let stream = connect(&addrs[..], TIMEOUT).unwrap();
        assert_eq!(stream.peer_addr().unwrap(), open.local_addr().unwrap());
    }

    #[test]
    fn the_last_failure_is_reported() {
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let err = connect(closed, TIMEOUT).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
        let none: &[SocketAddr] = &[];
        assert_eq!(
            connect(none, TIMEOUT).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
