// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

// iPadOS brings the iPhone's USB network up itself but gives apps no usbmuxd. carkit is reached
// there through the phone's service directory (RSD) on that link: remote pairing over its
// untrusted tunnel service, then a TLS-PSK tunnel. Apps get no utun, so TCP inside the tunnel
// runs on idevice's userspace stack.

use std::fmt::Display;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use idevice::remote_pairing::{RemotePairingClient, RpPairingFile, connect_tls_psk_tunnel_native};
use idevice::tcp::adapter::Adapter;
use idevice::{Idevice, ReadWrite, RemoteXpcClient};
use tokio::net::TcpStream;

use crate::Error;

const TUNNEL_SERVICE: &str = "com.apple.internal.dt.coredevice.untrusted.tunnelservice";
const CARKIT_SHIM: &str = "com.apple.carkit.service.shim.remote";
const HOST_NAME: &str = "LIVI";
const STEP: Duration = Duration::from_secs(6);
const TRUST: Duration = Duration::from_secs(90);
// IPv6 and TCP headers within the tunnel's MTU.
const HEADERS: usize = 60;

pub struct RemoteCarkit {
    pub udid: String,
    pub stream: Box<dyn ReadWrite>,
}

struct Directory {
    udid: String,
    services: Vec<(String, u16)>,
}

impl Directory {
    fn port(&self, service: &str) -> Result<u16, Error> {
        self.services
            .iter()
            .find(|(name, _)| name == service)
            .map(|(_, port)| *port)
            .ok_or_else(|| Error::Remote(format!("{service} not offered")))
    }
}

fn failed(what: &str) -> impl FnOnce(idevice::IdeviceError) -> Error + '_ {
    move |e| Error::Remote(format!("{what}: {e}"))
}

async fn within<T, E: Display>(
    what: &str,
    wait: Duration,
    step: impl Future<Output = Result<T, E>>,
) -> Result<T, Error> {
    match tokio::time::timeout(wait, step).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(Error::Remote(format!("{what}: {e}"))),
        Err(_) => Err(Error::Remote(format!("{what} timed out"))),
    }
}

// remoted serves one connection at a time, so the one that found it reads the directory.
pub async fn open_carkit(remoted: TcpStream, store: &Path) -> Result<RemoteCarkit, Error> {
    let addr = remoted.peer_addr().map_err(Error::Store)?;
    let untrusted = directory(remoted).await?;
    let mut at = addr;
    at.set_port(untrusted.port(TUNNEL_SERVICE)?);
    let tunnel = tunnel(at, &record_file(store, &untrusted.udid)).await?;
    let (ours, theirs) = match (
        tunnel.info.client_address.parse(),
        tunnel.info.server_address.parse(),
    ) {
        (Ok(ours), Ok(theirs)) => (ours, theirs),
        _ => return Err(Error::Remote("tunnel addresses do not parse".into())),
    };
    let (rsd, mtu) = (tunnel.info.server_rsd_port, tunnel.info.mtu as usize);
    let inner: Box<dyn ReadWrite> = Box::new(tunnel.into_inner());
    let mut adapter = Adapter::new(Box::new(inner), ours, theirs);
    adapter.set_mss(mtu.saturating_sub(HEADERS));
    // Every stream holds the adapter task, which ends with the last of them.
    let mut handle = adapter.to_async_handle();
    let trusted =
        directory(within("trusted rsd connect", STEP, handle.connect(rsd)).await?).await?;
    let shim = within(
        "carkit connect",
        STEP,
        handle.connect(trusted.port(CARKIT_SHIM)?),
    )
    .await?;
    let mut carkit = Idevice::new(Box::new(shim), "livi-carkit");
    within("carkit checkin", STEP, carkit.rsd_checkin()).await?;
    let stream = carkit
        .get_socket()
        .ok_or_else(|| Error::Remote("carkit stream unavailable".into()))?;
    Ok(RemoteCarkit {
        udid: untrusted.udid,
        stream,
    })
}

fn record_file(store: &Path, udid: &str) -> PathBuf {
    store.join("remote").join(format!("{udid}.plist"))
}

async fn tunnel(
    at: SocketAddr,
    record_path: &Path,
) -> Result<idevice::remote_pairing::CdTunnel<impl ReadWrite + use<>>, Error> {
    let mut record = match RpPairingFile::read_from_file(record_path).await {
        Ok(record) => record,
        Err(_) => RpPairingFile::generate(HOST_NAME),
    };
    let socket = within("tunnel service connect", STEP, TcpStream::connect(at)).await?;
    let mut xpc = within("tunnel service", STEP, RemoteXpcClient::new(socket)).await?;
    within("tunnel service handshake", STEP, xpc.do_handshake()).await?;
    // The tunnel service opens with its ServiceVersion, which pairing would take for its reply.
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(2), xpc.recv_root()).await {}
    let mut client = RemotePairingClient::new(xpc, HOST_NAME);
    println!("[wired] pairing over the USB link, confirm the trust dialog on the phone");
    // The phone asks for trust, it shows no PIN to enter.
    within(
        "pairing",
        TRUST,
        client.connect(&mut record, || async { "000000".to_string() }),
    )
    .await?;
    if let Some(dir) = record_path.parent() {
        std::fs::create_dir_all(dir).map_err(Error::Store)?;
    }
    record
        .write_to_file(record_path)
        .await
        .map_err(failed("save pair record"))?;
    let mut listener = at;
    listener.set_port(
        client
            .create_tcp_listener()
            .await
            .map_err(failed("tunnel listener"))?,
    );
    let stream = within("tunnel connect", STEP, TcpStream::connect(listener)).await?;
    within(
        "tunnel",
        STEP,
        connect_tls_psk_tunnel_native(stream, client.encryption_key()),
    )
    .await
}

// RSD sends its directory after the XPC handshake, some peers only after the device handshake.
async fn directory<S: ReadWrite>(socket: S) -> Result<Directory, Error> {
    let mut xpc = within("rsd", STEP, RemoteXpcClient::new(socket)).await?;
    within("rsd handshake", STEP, xpc.do_handshake()).await?;
    let root = match tokio::time::timeout(STEP, xpc.recv_root()).await {
        Ok(root) => root.map_err(failed("rsd"))?,
        Err(_) => {
            xpc.send_device_handshake()
                .await
                .map_err(failed("rsd device handshake"))?;
            within("rsd", STEP, xpc.recv_root()).await?
        }
    };
    let root = root.as_dictionary();
    let udid = root
        .and_then(|d| d.get("Properties"))
        .and_then(|p| p.as_dictionary())
        .and_then(|p| p.get("UniqueDeviceID"))
        .and_then(|u| u.as_string())
        .ok_or_else(|| Error::Remote("rsd names no device".into()))?
        .to_string();
    let services = root
        .and_then(|d| d.get("Services"))
        .and_then(|s| s.as_dictionary())
        .map(|services| {
            services
                .iter()
                .filter_map(|(name, service)| {
                    let port = service.as_dictionary()?.get("Port")?.as_string()?;
                    Some((name.clone(), port.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Directory { udid, services })
}
