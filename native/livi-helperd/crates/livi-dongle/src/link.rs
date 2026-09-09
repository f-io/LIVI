//! The LIVI Link on the network: the dongle answers to `LINK_NAME` over mDNS on its USB link,
//! and everything on it is addressed by that name.

use std::net::ToSocketAddrs;

/// The name the dongle's mDNS responder answers to.
pub const LINK_NAME: &str = "livi-link.local";

/// What the Wi-Fi interface and Bluetooth adapter settings carry when a dongle radio is chosen.
pub const CHOICE: &str = "livi-link";

/// Whether the name resolves to an IPv4 address right now.
pub fn resolves() -> bool {
    (LINK_NAME, 0u16)
        .to_socket_addrs()
        .map(|mut a| a.any(|a| a.is_ipv4()))
        .unwrap_or(false)
}

/// "LINK_NAME:port".
pub fn addr(port: u16) -> String {
    format!("{LINK_NAME}:{port}")
}
