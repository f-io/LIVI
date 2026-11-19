// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::{Ipv6Addr, SocketAddrV6};

use crate::bplist::Value;

#[cfg(feature = "url-video")]
mod tunnel;

#[cfg_attr(not(feature = "url-video"), allow(dead_code))]
pub struct ProxyOffer {
    at: SocketAddrV6,
    sni: String,
    psk: Vec<u8>,
    identity: Vec<u8>,
    authorization: String,
}

impl ProxyOffer {
    pub fn from_params(params: &Value, scope: u32) -> Option<Self> {
        let url = params.get("proxyUrl")?.as_str()?;
        let sni = url
            .strip_prefix("https://")
            .unwrap_or(url)
            .trim_end_matches('/');
        let ip: Ipv6Addr = sni
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .ok()?;
        let port = u16::try_from(params.get("proxyPort")?.as_int()?).ok()?;
        Some(Self {
            at: SocketAddrV6::new(ip, port, 0, scope),
            sni: sni.to_string(),
            psk: params.get("proxyPsk")?.as_data()?.to_vec(),
            identity: params.get("proxyPskIdentity")?.as_data()?.to_vec(),
            authorization: params.get("proxyAuthorization")?.as_str()?.to_string(),
        })
    }

    pub fn address(&self) -> SocketAddrV6 {
        self.at
    }
}

#[cfg(not(feature = "url-video"))]
impl ProxyOffer {
    pub(crate) async fn connect(
        &self,
        _target: &str,
    ) -> std::io::Result<(tokio::net::TcpStream, Vec<u8>)> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}
