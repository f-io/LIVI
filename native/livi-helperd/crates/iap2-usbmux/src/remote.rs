// Client for the dongle's `livi-usbproxy`.
// Protocol (one connection per command, text request, then a text reply or the raw pipe):
//   LIST                -> "<serial> <num_configs> <config_value|->" per line, then "END"
//   CONFIG <serial>     -> "OK <num_configs> <config_value>" | "ERR <msg>"
//   RESTORE <serial>    -> "OK" | "ERR <msg>"
//   ATTACH <serial>     -> "OK", after which the connection is the bulk pipe in both directions

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::pipe::{MuxPipes, MuxReader, MuxWriter, PhoneInfo};

pub const DEFAULT_PORT: u16 = 5003;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Connects with CONNECT_TIMEOUT rather than the OS default.
fn connect(addr: &str) -> Result<TcpStream, String> {
    let sockaddr = addr
        .to_socket_addrs()
        .map_err(|e| format!("usbproxy {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("usbproxy {addr}: no address"))?;
    let s = TcpStream::connect_timeout(&sockaddr, CONNECT_TIMEOUT)
        .map_err(|e| format!("usbproxy {addr}: {e}"))?;
    s.set_nodelay(true).ok();
    Ok(s)
}

/// Reports the first failure and the recovery, not every scan.
fn note(result: &Result<Vec<String>, String>) {
    static DOWN: AtomicBool = AtomicBool::new(false);
    match result {
        Err(e) if !DOWN.swap(true, Ordering::Relaxed) => eprintln!("[usbproxy] unreachable: {e}"),
        Ok(_) if DOWN.swap(false, Ordering::Relaxed) => println!("[usbproxy] reachable again"),
        _ => {}
    }
}

/// Sends one command and returns the reply lines up to "END" (or the single status line).
fn command(addr: &str, cmd: &str, want_lines: bool) -> Result<Vec<String>, String> {
    let mut s = connect(addr)?;
    s.set_read_timeout(Some(Duration::from_secs(15))).ok();
    s.write_all(format!("{cmd}\n").as_bytes()).map_err(|e| format!("usbproxy write: {e}"))?;
    let mut r = BufReader::new(s);
    let mut out = Vec::new();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).map_err(|e| format!("usbproxy read: {e}"))? == 0 {
            break;
        }
        let line = line.trim_end().to_string();
        if line == "END" {
            break;
        }
        if let Some(msg) = line.strip_prefix("ERR ") {
            return Err(msg.to_string());
        }
        out.push(line);
        if !want_lines {
            break;
        }
    }
    Ok(out)
}

pub fn find_iphones(addr: &str) -> Vec<PhoneInfo> {
    try_find_iphones(addr).unwrap_or_default()
}

/// Separates "the proxy did not answer" from "it answered with no phone".
pub fn try_find_iphones(addr: &str) -> Result<Vec<PhoneInfo>, String> {
    let result = command(addr, "LIST", true);
    note(&result);
    let lines = result?;
    Ok(lines
        .iter()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let serial = f.next()?.to_string();
            let num_configs = f.next()?.parse().ok()?;
            let config_value = f.next().and_then(|v| v.parse().ok());
            Some(PhoneInfo { serial, num_configs, config_value })
        })
        .collect())
}

pub fn ensure_carplay_config(addr: &str, serial: &str) -> Result<PhoneInfo, String> {
    let lines = command(addr, &format!("CONFIG {serial}"), false)?;
    let line = lines.first().ok_or("usbproxy: empty CONFIG reply")?;
    let mut f = line.split_whitespace();
    if f.next() != Some("OK") {
        return Err(format!("usbproxy CONFIG: {line}"));
    }
    Ok(PhoneInfo {
        serial: serial.to_string(),
        num_configs: f.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        config_value: f.next().and_then(|v| v.parse().ok()),
    })
}

pub fn restore_default_config(addr: &str, serial: &str) {
    let _ = command(addr, &format!("RESTORE {serial}"), false);
}

struct TcpWriter(TcpStream);
struct TcpReader(TcpStream);

impl MuxWriter for TcpWriter {
    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.0.write_all(data).map_err(|e| format!("usbproxy pipe write: {e}"))
    }
}

impl MuxReader for TcpReader {
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        self.0.set_read_timeout(Some(timeout)).ok();
        let mut buf = vec![0u8; 65536];
        match self.0.read(&mut buf) {
            Ok(0) => Err("usbproxy pipe closed".into()),
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) => Err(format!("usbproxy pipe read: {e}")),
        }
    }
}

/// Attaches to the phone's bulk endpoints; the socket then carries raw mux bytes.
pub fn open_pipes(
    addr: &str,
    serial: &str,
) -> Result<MuxPipes, String> {
    let mut s = connect(addr)?;
    s.set_read_timeout(Some(Duration::from_secs(20))).ok();
    s.write_all(format!("ATTACH {serial}\n").as_bytes())
        .map_err(|e| format!("usbproxy write: {e}"))?;

    // Read only the status line; pipe bytes that follow stay in the socket.
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match s.read(&mut byte) {
            Ok(0) => return Err("usbproxy closed during ATTACH".into()),
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                line.push(byte[0]);
                if line.len() > 256 {
                    return Err("usbproxy ATTACH reply too long".into());
                }
            }
            Err(e) => return Err(format!("usbproxy ATTACH: {e}")),
        }
    }
    let status = String::from_utf8_lossy(&line).trim_end().to_string();
    if status != "OK" {
        return Err(format!("usbproxy ATTACH: {status}"));
    }

    let reader = s.try_clone().map_err(|e| format!("usbproxy clone: {e}"))?;
    Ok((Box::new(TcpWriter(s)), Box::new(TcpReader(reader))))
}
