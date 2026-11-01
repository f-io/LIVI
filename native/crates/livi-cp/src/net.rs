// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{TcpListener, UdpSocket};

fn any(kind: Type, proto: Protocol) -> io::Result<Socket> {
    let sock = Socket::new(Domain::IPV6, kind, Some(proto))?;
    sock.set_only_v6(false)?;
    sock.set_nonblocking(true)?;
    sock.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)).into())?;
    Ok(sock)
}

pub fn tcp_listener() -> io::Result<TcpListener> {
    let sock = any(Type::STREAM, Protocol::TCP)?;
    sock.listen(16)?;
    TcpListener::from_std(sock.into())
}

pub fn udp_socket() -> io::Result<UdpSocket> {
    UdpSocket::from_std(any(Type::DGRAM, Protocol::UDP)?.into())
}

pub fn v6(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(a) => {
            SocketAddr::V6(SocketAddrV6::new(a.ip().to_ipv6_mapped(), a.port(), 0, 0))
        }
        v6 => v6,
    }
}

pub fn local_port(addr: io::Result<SocketAddr>) -> u16 {
    addr.map(|a| a.port()).unwrap_or(0)
}

pub fn norm_host(host: &str) -> String {
    let host = host.split('%').next().unwrap_or("");
    match host.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("::ffff:") => host[7..].to_string(),
        _ => host.to_string(),
    }
}

pub fn host_of(addr: &SocketAddr) -> String {
    match addr {
        SocketAddr::V4(a) => a.ip().to_string(),
        SocketAddr::V6(a) => match a.ip().to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => a.ip().to_string(),
        },
    }
}

pub fn route_of(addr: &SocketAddr) -> String {
    match addr {
        SocketAddr::V6(a) if a.scope_id() != 0 && a.ip().to_ipv4_mapped().is_none() => {
            format!("{}%{}", a.ip(), a.scope_id())
        }
        _ => host_of(addr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_compare_without_zone_or_mapping() {
        assert_eq!(norm_host("::ffff:192.168.1.2"), "192.168.1.2");
        assert_eq!(norm_host("::FFFF:10.0.0.1"), "10.0.0.1");
        assert_eq!(norm_host("fe80::1%wlan0"), "fe80::1");
        assert_eq!(norm_host(""), "");
        let mapped: SocketAddr = "[::ffff:10.0.0.2]:5000".parse().unwrap();
        assert_eq!(host_of(&mapped), "10.0.0.2");
        let link_local: SocketAddr = "[fe80::2%3]:5000".parse().unwrap();
        assert_eq!(host_of(&link_local), "fe80::2");
        let v4: SocketAddr = "10.0.0.3:1".parse().unwrap();
        assert_eq!(host_of(&v4), "10.0.0.3");
        assert_eq!(v6(v4), "[::ffff:10.0.0.3]:1".parse().unwrap());
        assert_eq!(v6(v6_addr()), v6_addr());
    }

    fn v6_addr() -> SocketAddr {
        "[fe80::2]:7".parse().unwrap()
    }

    #[test]
    fn a_link_local_route_keeps_its_interface() {
        let link_local: SocketAddr = "[fe80::2%3]:5000".parse().unwrap();
        assert_eq!(route_of(&link_local), "fe80::2%3");
        assert_eq!(route_of(&v6_addr()), "fe80::2");
        let mapped: SocketAddr = "[::ffff:10.0.0.2]:5000".parse().unwrap();
        assert_eq!(route_of(&mapped), "10.0.0.2");
        assert_eq!(route_of(&"10.0.0.3:1".parse().unwrap()), "10.0.0.3");
    }

    #[tokio::test]
    async fn sockets_take_both_families() {
        let tcp = tcp_listener().unwrap();
        let port = local_port(tcp.local_addr());
        assert!(port > 0);
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let udp = udp_socket().unwrap();
        assert!(local_port(udp.local_addr()) > 0);
        assert_eq!(local_port(Err(io::Error::other("x"))), 0);
    }
}
