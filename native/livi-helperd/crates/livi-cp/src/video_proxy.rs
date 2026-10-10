//! The proxy the phone offers in setProxyParameters, the car's only way to content that lives on
//! the phone, such as a video app's own HLS server on 127.0.0.1. The phone speaks TLS 1.3 with an
//! external PSK and the AES-256/SHA-384 suite only, then HTTP CONNECT. A local listener hands
//! every connection through its own tunnel, so a player only ever sees plain HTTP on 127.0.0.1.
//! Only http:// content goes through, LIVI plays no DRM content.

use std::ffi::{c_int, c_uchar};
use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use foreign_types::ForeignType;
use openssl::ex_data::Index;
use openssl::ssl::{Ssl, SslContext, SslMethod, SslVerifyMode, SslVersion};
use openssl_sys as ffi;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tokio_openssl::SslStream;

use crate::bplist::Value;

const SUITE: &str = "TLS_AES_256_GCM_SHA384";
const SUITE_ID: [u8; 2] = [0x13, 0x02];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEAD: usize = 8192;

type UseSession = unsafe extern "C" fn(
    *mut ffi::SSL,
    *const ffi::EVP_MD,
    *mut *const c_uchar,
    *mut usize,
    *mut *mut ffi::SSL_SESSION,
) -> c_int;

unsafe extern "C" {
    fn SSL_set_psk_use_session_callback(ssl: *mut ffi::SSL, cb: Option<UseSession>);
    fn SSL_SESSION_new() -> *mut ffi::SSL_SESSION;
    fn SSL_SESSION_set1_master_key(
        s: *mut ffi::SSL_SESSION,
        key: *const c_uchar,
        len: usize,
    ) -> c_int;
    fn SSL_SESSION_set_cipher(s: *mut ffi::SSL_SESSION, cipher: *const ffi::SSL_CIPHER) -> c_int;
    fn SSL_SESSION_set_protocol_version(s: *mut ffi::SSL_SESSION, version: c_int) -> c_int;
    fn SSL_CIPHER_find(ssl: *mut ffi::SSL, ptr: *const c_uchar) -> *const ffi::SSL_CIPHER;
}

struct Psk {
    key: Vec<u8>,
    identity: Vec<u8>,
}

fn psk_index() -> Result<Index<Ssl, Psk>, openssl::error::ErrorStack> {
    static INDEX: OnceLock<Index<Ssl, Psk>> = OnceLock::new();
    if let Some(index) = INDEX.get() {
        return Ok(*index);
    }
    let index = Ssl::new_ex_index()?;
    Ok(*INDEX.get_or_init(|| index))
}

/// A session made of the PSK alone, the shape OpenSSL wants an external PSK in.
unsafe fn psk_session(ssl: *mut ffi::SSL, psk: &Psk) -> *mut ffi::SSL_SESSION {
    unsafe {
        let cipher = SSL_CIPHER_find(ssl, SUITE_ID.as_ptr());
        let session = SSL_SESSION_new();
        if cipher.is_null() || session.is_null() {
            return std::ptr::null_mut();
        }
        if SSL_SESSION_set1_master_key(session, psk.key.as_ptr(), psk.key.len()) != 1
            || SSL_SESSION_set_cipher(session, cipher) != 1
            || SSL_SESSION_set_protocol_version(session, ffi::TLS1_3_VERSION) != 1
        {
            ffi::SSL_SESSION_free(session);
            return std::ptr::null_mut();
        }
        session
    }
}

/// OpenSSL asks once before the ClientHello without a digest, and again after a retry with the
/// digest the server picked. A PSK made for SHA-384 must not be offered for another digest.
unsafe extern "C" fn use_session(
    ssl: *mut ffi::SSL,
    md: *const ffi::EVP_MD,
    id: *mut *const c_uchar,
    id_len: *mut usize,
    session: *mut *mut ffi::SSL_SESSION,
) -> c_int {
    let Ok(index) = psk_index() else { return 0 };
    unsafe {
        let psk = ffi::SSL_get_ex_data(ssl, index.as_raw()) as *const Psk;
        let cipher = SSL_CIPHER_find(ssl, SUITE_ID.as_ptr());
        if psk.is_null() || cipher.is_null() {
            return 0;
        }
        if !md.is_null() && ffi::SSL_CIPHER_get_handshake_digest(cipher) != md {
            *session = std::ptr::null_mut();
            return 1;
        }
        let made = psk_session(ssl, &*psk);
        if made.is_null() {
            return 0;
        }
        *session = made;
        *id = (*psk).identity.as_ptr();
        *id_len = (*psk).identity.len();
    }
    1
}

/// What the phone hands over in setProxyParameters.
pub struct ProxyOffer {
    at: SocketAddrV6,
    sni: String,
    psk: Vec<u8>,
    identity: Vec<u8>,
    authorization: String,
}

impl ProxyOffer {
    /// The proxy sits on the phone's link-local address, `scope` is the interface the phone
    /// reaches LIVI on.
    pub fn from_params(params: &Value, scope: u32) -> Option<Self> {
        let url = params.get("proxyUrl")?.as_str()?;
        let sni = url.strip_prefix("https://").unwrap_or(url).trim_end_matches('/');
        let ip: Ipv6Addr = sni.trim_start_matches('[').trim_end_matches(']').parse().ok()?;
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

    fn tls(&self) -> io::Result<Ssl> {
        let mut ctx = SslContext::builder(SslMethod::tls_client()).map_err(io::Error::other)?;
        ctx.set_min_proto_version(Some(SslVersion::TLS1_3)).map_err(io::Error::other)?;
        ctx.set_ciphersuites(SUITE).map_err(io::Error::other)?;
        // The PSK authenticates the phone, it shows no certificate.
        ctx.set_verify(SslVerifyMode::NONE);
        let mut ssl = Ssl::new(&ctx.build()).map_err(io::Error::other)?;
        ssl.set_hostname(&self.sni).map_err(io::Error::other)?;
        let psk = Psk { key: self.psk.clone(), identity: self.identity.clone() };
        ssl.set_ex_data(psk_index().map_err(io::Error::other)?, psk);
        unsafe { SSL_set_psk_use_session_callback(ssl.as_ptr(), Some(use_session)) };
        Ok(ssl)
    }

    /// A connection to `target` (host:port as the phone sees it) through the proxy, plus what
    /// came after the CONNECT answer.
    async fn connect(&self, target: &str) -> io::Result<(SslStream<TcpStream>, Vec<u8>)> {
        let tcp =
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(SocketAddr::V6(self.at)))
                .await
                .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        let mut tls = SslStream::new(self.tls()?, tcp).map_err(io::Error::other)?;
        Pin::new(&mut tls).connect().await.map_err(io::Error::other)?;
        let ask = format!(
            "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Authorization: {}\r\n\r\n",
            self.authorization
        );
        tls.write_all(ask.as_bytes()).await?;
        let mut head = Vec::new();
        let end = loop {
            if let Some(at) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
            if head.len() > MAX_HEAD {
                return Err(io::Error::other("CONNECT answer without end"));
            }
            let mut chunk = [0u8; 1024];
            let n = tls.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            head.extend_from_slice(&chunk[..n]);
        };
        let line = head.split(|&b| b == b'\r').next().unwrap_or_default();
        if line.split(|&b| b == b' ').nth(1) != Some(b"200") {
            let line = String::from_utf8_lossy(line).into_owned();
            return Err(io::Error::other(format!("proxy refused: {line}")));
        }
        Ok((tls, head.split_off(end)))
    }
}

/// host:port and path of a plain http URL. Anything else, https above all, is left alone.
pub fn plain_http(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = rest.find('/').map_or((rest, "/"), |at| (&rest[..at], &rest[at..]));
    if host.is_empty() {
        return None;
    }
    let has_port = match host.rfind(']') {
        Some(bracket) => host[bracket..].contains(':'),
        None => host.contains(':'),
    };
    let target = if has_port { host.to_string() } else { format!("{host}:80") };
    Some((target, path.to_string()))
}

/// A listener on 127.0.0.1 whose every connection reaches one target on the phone.
pub struct Tunnel {
    url: String,
    task: JoinHandle<()>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Tunnel {
    /// None when `content` is not plain http.
    pub async fn open(offer: Arc<ProxyOffer>, content: &str) -> io::Result<Option<Self>> {
        let Some((target, path)) = plain_http(content) else { return Ok(None) };
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://{}{path}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            // Dropping the set with the aborted task ends every open relay too.
            let mut relays = JoinSet::new();
            while let Ok((local, _)) = listener.accept().await {
                relays.spawn(relay(offer.clone(), target.clone(), local));
                while relays.try_join_next().is_some() {}
            }
        });
        Ok(Some(Self { url, task }))
    }

    /// Where a player on this machine fetches the content.
    pub fn url(&self) -> &str {
        &self.url
    }
}

async fn relay(offer: Arc<ProxyOffer>, target: String, mut local: TcpStream) {
    let (mut tls, early) = match offer.connect(&target).await {
        Ok(up) => up,
        Err(e) => {
            println!("[videoProxy] no way to {target}: {e}");
            return;
        }
    };
    if local.write_all(&early).await.is_ok() {
        let _ = tokio::io::copy_bidirectional(&mut local, &mut tls).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bplist::dict;

    const KEY: [u8; 32] = [7; 32];
    const IDENTITY: &[u8] = b"identity-1";
    const AUTH: &str = "Basic dXNlcjpwYXNz";

    type FindSession = unsafe extern "C" fn(
        *mut ffi::SSL,
        *const c_uchar,
        usize,
        *mut *mut ffi::SSL_SESSION,
    ) -> c_int;

    unsafe extern "C" {
        fn SSL_set_psk_find_session_callback(ssl: *mut ffi::SSL, cb: Option<FindSession>);
    }

    unsafe extern "C" fn find_session(
        ssl: *mut ffi::SSL,
        id: *const c_uchar,
        id_len: usize,
        session: *mut *mut ffi::SSL_SESSION,
    ) -> c_int {
        unsafe {
            if std::slice::from_raw_parts(id, id_len) != IDENTITY {
                *session = std::ptr::null_mut();
                return 1;
            }
            let psk = Psk { key: KEY.to_vec(), identity: IDENTITY.to_vec() };
            *session = psk_session(ssl, &psk);
            c_int::from(!(*session).is_null())
        }
    }

    /// Stands in for the phone: the PSK proxy and, behind it, an HTTP server that names the path.
    async fn phone() -> SocketAddrV6 {
        let listener = TcpListener::bind("[::1]:0").await.unwrap();
        let SocketAddr::V6(at) = listener.local_addr().unwrap() else { unreachable!() };
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut ctx = SslContext::builder(SslMethod::tls_server()).unwrap();
                    ctx.set_min_proto_version(Some(SslVersion::TLS1_3)).unwrap();
                    ctx.set_ciphersuites(SUITE).unwrap();
                    let ssl = Ssl::new(&ctx.build()).unwrap();
                    unsafe { SSL_set_psk_find_session_callback(ssl.as_ptr(), Some(find_session)) };
                    let mut tls = SslStream::new(ssl, tcp).unwrap();
                    if Pin::new(&mut tls).accept().await.is_err() {
                        return;
                    }
                    let ask = read_head(&mut tls).await;
                    if !ask.starts_with("CONNECT 127.0.0.1:58539 ")
                        || !ask.contains(&format!("Proxy-Authorization: {AUTH}\r\n"))
                    {
                        let _ = tls.write_all(b"HTTP/1.1 407 Nope\r\n\r\n").await;
                        return;
                    }
                    tls.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
                    let get = read_head(&mut tls).await;
                    let path = get.split(' ').nth(1).unwrap_or_default().to_string();
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",
                        path.len()
                    );
                    tls.write_all(answer.as_bytes()).await.unwrap();
                    let _ = tls.shutdown().await;
                });
            }
        });
        at
    }

    async fn read_head(tls: &mut SslStream<TcpStream>) -> String {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0u8; 1];
            if tls.read(&mut b).await.unwrap_or(0) == 0 {
                break;
            }
            head.push(b[0]);
        }
        String::from_utf8(head).unwrap()
    }

    fn params(at: SocketAddrV6, auth: &str) -> Value {
        dict([
            ("proxyUrl", Value::String(format!("https://[{}]", at.ip()))),
            ("proxyPort", Value::Int(u64::from(at.port()))),
            ("proxyPsk", Value::Data(KEY.to_vec())),
            ("proxyPskIdentity", Value::Data(IDENTITY.to_vec())),
            ("proxyAuthorization", Value::String(auth.into())),
        ])
    }

    async fn get(url: &str) -> String {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_at(rest.find('/').unwrap());
        let mut sock = TcpStream::connect(host).await.unwrap();
        sock.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut answer = String::new();
        let _ = sock.read_to_string(&mut answer).await;
        answer
    }

    #[test]
    fn the_offer_is_read_from_set_proxy_parameters() {
        let at = SocketAddrV6::new("fe80::88:6ad0:5dab:f30a".parse().unwrap(), 60664, 0, 0);
        let offer = ProxyOffer::from_params(&params(at, AUTH), 3).unwrap();
        assert_eq!(offer.address(), SocketAddrV6::new(*at.ip(), 60664, 0, 3));
        assert_eq!(offer.sni, "[fe80::88:6ad0:5dab:f30a]");
        assert_eq!((offer.psk.as_slice(), offer.identity.as_slice()), (&KEY[..], IDENTITY));
        assert!(ProxyOffer::from_params(&dict([("proxyPort", Value::Int(1))]), 0).is_none());
        let mut bad = params(at, AUTH);
        if let Value::Dict(entries) = &mut bad {
            entries[0].1 = Value::String("https://phone.local".into());
        }
        assert!(ProxyOffer::from_params(&bad, 0).is_none());
    }

    #[test]
    fn only_plain_http_goes_through() {
        let pair = |t: &str, p: &str| Some((t.to_string(), p.to_string()));
        assert_eq!(
            plain_http("http://127.0.0.1:58539/mirror/index.m3u8"),
            pair("127.0.0.1:58539", "/mirror/index.m3u8")
        );
        assert_eq!(plain_http("http://phone.local"), pair("phone.local:80", "/"));
        assert_eq!(plain_http("http://[::1]:8080/a"), pair("[::1]:8080", "/a"));
        assert_eq!(plain_http("http://[::1]/a"), pair("[::1]:80", "/a"));
        assert_eq!(plain_http("https://play.itunes.apple.com/x.m3u8"), None);
        assert_eq!(plain_http("http:///x"), None);
    }

    #[tokio::test]
    async fn a_player_reaches_the_phone_through_the_tunnel() {
        let at = phone().await;
        let offer = Arc::new(ProxyOffer::from_params(&params(at, AUTH), 0).unwrap());
        let tunnel = Tunnel::open(offer.clone(), "http://127.0.0.1:58539/mirror/index.m3u8")
            .await
            .unwrap()
            .unwrap();
        assert!(tunnel.url().starts_with("http://127.0.0.1:"));
        assert!(tunnel.url().ends_with("/mirror/index.m3u8"));
        let answer = get(tunnel.url()).await;
        assert!(answer.starts_with("HTTP/1.1 200 OK") && answer.ends_with("/mirror/index.m3u8"));
        let again = get(&tunnel.url().replace("index.m3u8", "part1.mp4")).await;
        assert!(again.ends_with("/mirror/part1.mp4"), "{again}");

        assert!(Tunnel::open(offer, "https://x/a.m3u8").await.unwrap().is_none());
        let url = tunnel.url().to_string();
        drop(tunnel);
        tokio::task::yield_now().await;
        let host = url.trim_start_matches("http://").split('/').next().unwrap().to_string();
        assert!(TcpStream::connect(host).await.is_err());
    }

    #[tokio::test]
    async fn a_refused_tunnel_closes_the_player_connection() {
        let at = phone().await;
        let wrong = Arc::new(ProxyOffer::from_params(&params(at, "Basic d3Jvbmc="), 0).unwrap());
        let tunnel =
            Tunnel::open(wrong.clone(), "http://127.0.0.1:58539/a").await.unwrap().unwrap();
        assert_eq!(get(tunnel.url()).await, "");
        let err = wrong.connect("127.0.0.1:58539").await.err().unwrap();
        assert!(err.to_string().contains("407"), "{err}");

        let mut other_key = params(at, AUTH);
        if let Value::Dict(entries) = &mut other_key {
            entries[3].1 = Value::Data(b"someone-else".to_vec());
        }
        let stranger = ProxyOffer::from_params(&other_key, 0).unwrap();
        assert!(stranger.connect("127.0.0.1:58539").await.is_err());
    }
}
