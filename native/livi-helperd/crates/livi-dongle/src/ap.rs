//! The dongle's access point, asked over its control port.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::link;

pub const PORT: u16 = 5001;
const TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(500);

/// One field of `status`, or None when the dongle does not answer.
pub fn status_field(key: &str) -> Option<String> {
    let addr = (link::LINK_NAME, PORT).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.write_all(b"status\n").ok()?;
    let want = format!("{key} ");
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        if line == "ok" || line.starts_with("error") {
            break;
        }
        if let Some(value) = line.strip_prefix(&want) {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Waits until the access point is on
pub fn ready(within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    loop {
        if status_field("state").as_deref() == Some("on") {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// The name the access point is really carrying, which is what the phone must look for.
pub fn ssid() -> Option<String> {
    status_field("ssid")
}

/// The MAC the phone is told to look for.
pub fn mac() -> Option<String> {
    status_field("mac")
}

/// The dongle's Bluetooth address, six bytes, most significant first.
pub fn bt_mac() -> Option<[u8; 6]> {
    let text = status_field("btmac")?;
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in &mut out {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}
