// The TCP listener a phone reaches after the WPP bootstrap. Every connection
// gets a session socket (livi_session_io::sock) the main process is told about.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use livi_session_io::sock::bind_session_socket;

use crate::session::Peer;

const BIND_RETRY: Duration = Duration::from_secs(5);
const KEEPALIVE: Duration = Duration::from_secs(5);

pub async fn run(port: u16, on_session: impl Fn(&str, IpAddr) + Send + Sync + 'static) {
    let listener = loop {
        match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => break l,
            Err(e) => {
                eprintln!("[aa-tcp] bind {port}: {e}, retrying");
                tokio::time::sleep(BIND_RETRY).await;
            }
        }
    };
    println!("[aa-tcp] listening on {port}");

    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("[aa-tcp] accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if let Err(e) = tune(&tcp) {
            eprintln!("[aa-tcp] {peer}: socket options: {e}");
        }

        let Some((node, path)) = bind_session_socket("aa-session") else { continue };
        println!("[aa-tcp] connection from {peer}, session socket {path}");
        on_session(&path, peer.ip());
        let phone = Peer { label: peer.to_string(), ip: peer.ip() };
        tokio::spawn(async move {
            crate::session::run(tcp, phone, node, path, Arc::new(Notify::new())).await;
        });
    }
}

/// TCP keepalive: probes after 5 s idle.
fn tune(tcp: &TcpStream) -> std::io::Result<()> {
    tcp.set_nodelay(true)?;
    let sock = SockRef::from(tcp);
    sock.set_tcp_keepalive(&TcpKeepalive::new().with_time(KEEPALIVE).with_interval(KEEPALIVE))
}
