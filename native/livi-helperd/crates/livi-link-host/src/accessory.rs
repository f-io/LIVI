//! `accessoryd` on the dongle is the accessory: it pairs the phone, opens its channel and passes the
//! bytes on to this host.

use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::link;

const RETRY: Duration = Duration::from_secs(5);
/// A header line is well under this, so anything longer is not one.
const LINE_MAX: usize = 64;
/// The announcement is a handful of lines, never more.
const LINES_MAX: usize = 8;
/// Keepalive idle time and interval in seconds, then probes before giving up.
const KEEPALIVE_IDLE: libc::c_int = 5;
const KEEPALIVE_EVERY: libc::c_int = 3;
const KEEPALIVE_TRIES: libc::c_int = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Session {
    /// The phone's Bluetooth address.
    pub peer: String,
    /// The dongle's own, which is the one the phone believes it is talking to.
    pub local: [u8; 6],
    pub stream: TcpStream,
}

pub fn sessions(ready: impl Fn() -> bool + Send + 'static) -> mpsc::Receiver<Session> {
    sessions_on(livi_net::port::IAP, "iap", ready)
}

fn sessions_on(
    port: u16,
    tag: &'static str,
    ready: impl Fn() -> bool + Send + 'static,
) -> mpsc::Receiver<Session> {
    let (tx, rx) = mpsc::channel(2);
    tokio::spawn(async move {
        loop {
            if !ready() {
                tokio::time::sleep(RETRY).await;
                continue;
            }
            match waiting(port, tag).await {
                Ok(session) => {
                    if tx.send(session).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("[{tag}] {e}");
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    });
    rx
}

async fn waiting(port: u16, tag: &str) -> Result<Session, String> {
    let blocking = tokio::task::spawn_blocking(move || {
        livi_net::connect((link::LINK_NAME, port), CONNECT_TIMEOUT)
    })
    .await
    .map_err(|e| format!("dongle: {e}"))?
    .map_err(|e| format!("dongle: {e}"))?;
    blocking.set_nonblocking(true).map_err(|e| format!("dongle: {e}"))?;
    let mut stream = TcpStream::from_std(blocking).map_err(|e| format!("dongle: {e}"))?;
    stream.set_nodelay(true).map_err(|e| format!("nodelay: {e}"))?;
    // Waiting for a phone means a long silence, so keepalive has to notice a dongle that is gone.
    watch_liveness(&stream);
    let head = header(&mut stream).await?;
    let peer = head
        .iter()
        .find_map(|l| l.strip_prefix("peer "))
        .ok_or("the dongle named no phone")?
        .to_string();
    let local = head
        .iter()
        .find_map(|l| l.strip_prefix("local "))
        .and_then(address)
        .ok_or("the dongle named no controller")?;
    println!("[{tag}] {peer} is on the dongle's bluetooth");
    Ok(Session { peer, local, stream })
}

fn watch_liveness(stream: &TcpStream) {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    let set = |level: libc::c_int, name: libc::c_int, value: libc::c_int| unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &raw const value as *const libc::c_void,
            size_of::<libc::c_int>() as libc::socklen_t,
        );
    };
    set(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    #[cfg(target_os = "linux")]
    set(libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, KEEPALIVE_IDLE);
    #[cfg(target_os = "macos")]
    set(libc::IPPROTO_TCP, libc::TCP_KEEPALIVE, KEEPALIVE_IDLE);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, KEEPALIVE_EVERY);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPCNT, KEEPALIVE_TRIES);
}

/// Most significant byte first.
fn address(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in &mut out {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// Byte by byte, so no buffered read takes the session's own bytes.
async fn header(stream: &mut TcpStream) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for _ in 0..LINES_MAX {
        let line = read_line(stream).await?;
        if line.is_empty() {
            return Ok(lines);
        }
        lines.push(line);
    }
    Err("the dongle never finished its announcement".into())
}

async fn read_line(stream: &mut TcpStream) -> Result<String, String> {
    let mut line = Vec::new();
    loop {
        let byte = stream.read_u8().await.map_err(|e| format!("dongle: {e}"))?;
        if byte == b'\n' {
            return String::from_utf8(line).map_err(|e| format!("dongle: {e}"));
        }
        line.push(byte);
        if line.len() > LINE_MAX {
            return Err("the dongle never finished its line".into());
        }
    }
}

fn order(line: &str) -> Result<(), String> {
    crate::ap::order(&format!("accessory {line}"))
}

pub fn drop_link(mac: &str) -> Result<(), String> {
    order(&format!("disconnect {mac}"))
}

#[cfg(test)]
mod tests {
    use super::address;

    #[test]
    fn an_address_reads_in_the_order_it_is_written() {
        assert_eq!(address("38:BA:B0:A0:E6:6F"), Some([0x38, 0xba, 0xb0, 0xa0, 0xe6, 0x6f]));
        assert_eq!(address("38:BA:B0:A0:E6"), None);
        assert_eq!(address("38:BA:B0:A0:E6:6F:11"), None);
        assert_eq!(address("not an address"), None);
    }
}
