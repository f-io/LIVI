// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target<'a> {
    pub tls: bool,
    pub host: &'a str,
    pub path: &'a str,
}

impl<'a> Target<'a> {
    pub fn of(url: &'a str) -> io::Result<Self> {
        let (tls, rest) = match (url.strip_prefix("http://"), url.strip_prefix("https://")) {
            (Some(rest), _) => (false, rest),
            (_, Some(rest)) => (true, rest),
            _ => {
                let e = format!("{url}: neither http nor https");
                return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
            }
        };
        let (host, path) = rest
            .find('/')
            .map_or((rest, "/"), |at| (&rest[..at], &rest[at..]));
        if host.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{url}: no host"),
            ));
        }
        Ok(Self { tls, host, path })
    }

    pub fn origin(&self) -> String {
        format!(
            "{}://{}",
            if self.tls { "https" } else { "http" },
            self.host
        )
    }

    pub fn address(&self) -> String {
        let has_port = match self.host.rfind(']') {
            Some(bracket) => self.host[bracket..].contains(':'),
            None => self.host.contains(':'),
        };
        match (has_port, self.tls) {
            (true, _) => self.host.to_string(),
            (false, true) => format!("{}:443", self.host),
            (false, false) => format!("{}:80", self.host),
        }
    }

    pub(super) fn name(&self) -> &'a str {
        let host = match self.host.rfind(']') {
            Some(bracket) => &self.host[..bracket],
            None => self
                .host
                .rsplit_once(':')
                .map_or(self.host, |(name, _)| name),
        };
        host.trim_start_matches('[')
    }
}

pub(crate) trait Conn: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Conn for T {}

pub(super) fn system_trust() -> io::Result<SslConnector> {
    static TRUST: OnceLock<SslConnector> = OnceLock::new();
    if let Some(trust) = TRUST.get() {
        return Ok(trust.clone());
    }
    let mut trust = SslConnector::builder(SslMethod::tls_client()).map_err(io::Error::other)?;
    if cfg!(target_os = "macos") {
        trust
            .set_ca_file("/etc/ssl/cert.pem")
            .map_err(io::Error::other)?;
    }
    let trust = trust.build();
    Ok(TRUST.get_or_init(|| trust).clone())
}

#[derive(Default)]
pub struct Client {
    open: HashMap<String, Vec<Box<dyn Conn>>>,
    dialer: Dialer,
}

impl Client {
    pub fn through(phone: Arc<ProxyOffer>) -> Self {
        Self {
            dialer: Dialer {
                phone: Some(phone),
                trust: None,
            },
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn trusting(mut self, trust: SslConnector) -> Self {
        self.dialer.trust = Some(trust);
        self
    }

    pub async fn get(&mut self, url: &str) -> io::Result<Vec<u8>> {
        let target = Target::of(url)?;
        let open = self.take(&target.origin());
        let reused = open.is_some();
        match self.exchange(&target, open, url).await {
            Err(e) if reused && e.kind() != io::ErrorKind::TimedOut => {
                self.exchange(&target, None, url).await
            }
            other => other,
        }
    }

    pub(crate) fn take(&mut self, origin: &str) -> Option<Box<dyn Conn>> {
        self.open.get_mut(origin)?.pop()
    }

    pub(crate) fn keep(&mut self, origin: String, conn: Box<dyn Conn>) {
        self.open.entry(origin).or_default().push(conn);
    }

    pub(crate) fn dialer(&self) -> Dialer {
        self.dialer.clone()
    }

    async fn exchange(
        &mut self,
        target: &Target<'_>,
        open: Option<Box<dyn Conn>>,
        url: &str,
    ) -> io::Result<Vec<u8>> {
        let fetch = async {
            let mut sock = match open {
                Some(sock) => sock,
                None => self.dialer.connect(target).await?,
            };
            let ask = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\n\r\n",
                target.path, target.host
            );
            sock.write_all(ask.as_bytes()).await?;
            let (head, body, again) = read_answer(&mut sock, url).await?;
            Ok::<_, io::Error>((head, body, again.then_some(sock)))
        };
        let (head, body, sock) = tokio::time::timeout(FETCH_TIMEOUT, fetch)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, format!("{url}: timed out")))??;
        if let Some(sock) = sock {
            self.keep(target.origin(), sock);
        }
        if head.split(' ').nth(1) != Some("200") {
            let line = head.lines().next().unwrap_or_default().to_string();
            return Err(io::Error::other(format!("{url}: {line}")));
        }
        Ok(body)
    }
}

#[derive(Clone, Default)]
pub(crate) struct Dialer {
    phone: Option<Arc<ProxyOffer>>,
    trust: Option<SslConnector>,
}

impl Dialer {
    pub(crate) async fn connect(&self, target: &Target<'_>) -> io::Result<Box<dyn Conn>> {
        let plain: Box<dyn Conn> = match &self.phone {
            Some(phone) => {
                let (tunnel, early) = phone.connect(&target.address()).await?;
                if !early.is_empty() {
                    return Err(io::Error::other("the proxy spoke before the request"));
                }
                Box::new(tunnel)
            }
            None => Box::new(TcpStream::connect(target.address()).await?),
        };
        if !target.tls {
            return Ok(plain);
        }
        let trust = match &self.trust {
            Some(trust) => trust.clone(),
            None => system_trust()?,
        };
        let ssl = trust.configure().and_then(|c| c.into_ssl(target.name()));
        let mut tls =
            SslStream::new(ssl.map_err(io::Error::other)?, plain).map_err(io::Error::other)?;
        Pin::new(&mut tls)
            .connect()
            .await
            .map_err(io::Error::other)?;
        Ok(Box::new(tls))
    }
}

pub(super) async fn read_answer<S: AsyncRead + Unpin>(
    sock: &mut S,
    url: &str,
) -> io::Result<(String, Vec<u8>, bool)> {
    let mut all = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut ended = false;
    loop {
        if let Some((head, body)) = split_answer(&all) {
            let again = !head.contains("connection: close");
            if let Some(len) = content_length(&head) {
                if body.len() >= len {
                    return Ok((head, body[..len].to_vec(), again));
                }
            } else if head.contains("transfer-encoding: chunked") {
                if let Some(out) = dechunk(body) {
                    return Ok((head, out, again));
                }
            } else if ended {
                return Ok((head, body.to_vec(), false));
            }
            if ended {
                return Err(io::Error::other(format!("{url}: answer cut short")));
            }
        } else if ended {
            return Err(io::Error::other(format!("{url}: no HTTP answer")));
        }
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            ended = true;
        }
        all.extend_from_slice(&chunk[..n]);
    }
}

pub(crate) fn with_query(url: &str, query: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{query}")
}

pub(super) fn split_answer(all: &[u8]) -> Option<(String, &[u8])> {
    let end = all.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some((
        String::from_utf8_lossy(&all[..end]).to_ascii_lowercase(),
        &all[end + 4..],
    ))
}

pub(super) fn content_length(head: &str) -> Option<usize> {
    head.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
}

pub(super) fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|w| w == b"\r\n")?;
        let size = std::str::from_utf8(&body[..line_end])
            .ok()?
            .split(';')
            .next()?
            .trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        body = body.get(line_end + 2..)?;
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(..size)?);
        body = body.get(size + 2..)?;
    }
}
