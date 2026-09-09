//! The iAP session the dongle's own Bluetooth hands over. `iapd` on the dongle is the accessory,
//! pairs the phone and opens its channel, and passes the bytes on so the link runs up here.

use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::link;

pub const PORT: u16 = 5004;
/// Where the dongle takes orders about its accessory.
pub const CONTROL_PORT: u16 = 5005;
/// How long a dongle that is not answering is left alone.
const RETRY: Duration = Duration::from_secs(5);
/// A header line is well under this, so anything longer is not one.
const LINE_MAX: usize = 64;
/// The announcement is a handful of lines, never more.
const LINES_MAX: usize = 8;
/// How a quiet link is checked: after this long, that often, that many times before giving up.
const KEEPALIVE_IDLE: libc::c_int = 5;
const KEEPALIVE_EVERY: libc::c_int = 3;
const KEEPALIVE_TRIES: libc::c_int = 3;
/// One order to the dongle is a line out and a line back, nothing that should take long.
const ORDER_TIMEOUT: Duration = Duration::from_secs(3);

/// A phone that opened its channel on the dongle, and the stream that carries it.
pub struct Session {
    /// The phone's Bluetooth address.
    pub peer: String,
    /// The dongle's own, which is the one the phone believes it is talking to.
    pub local: [u8; 6],
    pub stream: TcpStream,
}

/// Offers every session the dongle hands over, waiting again as soon as one is taken.
pub fn sessions() -> mpsc::Receiver<Session> {
    let (tx, rx) = mpsc::channel(2);
    tokio::spawn(async move {
        loop {
            match waiting().await {
                Ok(session) => {
                    if tx.send(session).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("[iap] {e}");
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    });
    rx
}

/// Holds a connection open until the dongle says a phone is on it.
async fn waiting() -> Result<Session, String> {
    let mut stream = TcpStream::connect(link::addr(PORT))
        .await
        .map_err(|e| format!("dongle: {e}"))?;
    stream
        .set_nodelay(true)
        .map_err(|e| format!("nodelay: {e}"))?;
    // Waiting for a phone means a long silence, so the link itself has to say when the dongle is
    // gone. Without this a restarted dongle leaves us listening to nobody.
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
    println!("[iap] {peer} is on the dongle's bluetooth");
    Ok(Session {
        peer,
        local,
        stream,
    })
}

/// Asks the kernel to check a quiet link, so a dongle that went away is noticed within seconds.
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

/// Six bytes out of the way people write them, most significant first.
fn address(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in out.iter_mut().rev() {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// Reads the announcement one byte at a time up to its blank line, so the session's own bytes
/// stay untouched.
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

/// Drops the dongle's link to one phone, which is what BlueZ does on a host that has it.
pub fn drop_link(mac: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write as _};
    use std::net::ToSocketAddrs;
    let addr = (link::LINK_NAME, CONTROL_PORT)
        .to_socket_addrs()
        .map_err(|e| format!("dongle: {e}"))?
        .next()
        .ok_or("the dongle has no address")?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, ORDER_TIMEOUT)
        .map_err(|e| format!("dongle: {e}"))?;
    stream
        .set_read_timeout(Some(ORDER_TIMEOUT))
        .map_err(|e| format!("dongle: {e}"))?;
    writeln!(stream, "disconnect {mac}").map_err(|e| format!("dongle: {e}"))?;
    let mut answer = String::new();
    BufReader::new(&stream)
        .read_line(&mut answer)
        .map_err(|e| format!("dongle: {e}"))?;
    match answer.trim() {
        "ok" => Ok(()),
        other => Err(other.trim_start_matches("error ").to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::address;

    #[test]
    fn an_address_reads_back_the_way_the_wire_carries_it() {
        assert_eq!(
            address("38:BA:B0:A0:E6:6F"),
            Some([0x6f, 0xe6, 0xa0, 0xb0, 0xba, 0x38])
        );
        assert_eq!(address("38:BA:B0:A0:E6"), None);
        assert_eq!(address("38:BA:B0:A0:E6:6F:11"), None);
        assert_eq!(address("not an address"), None);
    }
}
