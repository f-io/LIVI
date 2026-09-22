use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::{BIND_SHELL_PORT, DONGLE_HOST};

/// Bind-shell control channel. One command, one response, one sentinel.
pub struct BindShell {
    stream: TcpStream,
    buf: String,
}

const SENTINEL: &str = "LIVI-END";

pub fn is_up() -> bool {
    let Ok(addr) = format!("{DONGLE_HOST}:{BIND_SHELL_PORT}").parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

impl BindShell {
    pub fn connect(timeout_secs: u64) -> Result<Self, String> {
        let addr: SocketAddr = format!("{DONGLE_HOST}:{BIND_SHELL_PORT}")
            .parse()
            .map_err(|e| format!("bad addr: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
                    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
                    return Ok(BindShell {
                        stream,
                        buf: String::new(),
                    });
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                Err(e) => return Err(format!("connect bind-shell: {e}")),
            }
        }
    }

    pub fn run(&mut self, cmd: &str) -> Result<String, String> {
        let line = format!("({cmd}); echo {SENTINEL}:$?\n");
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let mut tmp = [0u8; 4096];
        loop {
            let n = self.stream.read(&mut tmp).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("shell closed".into());
            }
            self.buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
            if let Some(idx) = self.buf.find(SENTINEL) {
                let before = self.buf[..idx].trim_end().to_string();
                let after = self.buf[idx + SENTINEL.len() + 1..].to_string();
                let (rc_str, rest) = after.split_once('\n').unwrap_or((&after, ""));
                let rc: i32 = rc_str.trim().parse().unwrap_or(-1);
                self.buf = rest.to_string();
                if rc != 0 {
                    return Err(format!("`{cmd}` exit {rc}: {before}"));
                }
                return Ok(before);
            }
        }
    }

    /// The bind-shell has raw stdin/stdout on the TCP socket
    pub fn stream_out(&mut self, src_cmd: &str, expected_size: u64) -> Result<Vec<u8>, String> {
        if !self.buf.is_empty() {
            return Err(format!(
                "shell buffer not empty before stream_out: {:?}",
                &self.buf[..self.buf.len().min(64)]
            ));
        }
        let line = format!("({src_cmd}) 2>/dev/null; printf '\\n{SENTINEL}:0\\n'\n");
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| format!("write: {e}"))?;

        let mut out = Vec::with_capacity(expected_size as usize);
        let mut buf = [0u8; 65536];
        while (out.len() as u64) < expected_size {
            let want = ((expected_size as usize) - out.len()).min(buf.len());
            let n = self
                .stream
                .read(&mut buf[..want])
                .map_err(|e| format!("data read: {e}"))?;
            if n == 0 {
                return Err(format!(
                    "shell closed after {} B (expected {expected_size})",
                    out.len()
                ));
            }
            out.extend_from_slice(&buf[..n]);
        }
        let _ = self.await_sentinel()?;
        Ok(out)
    }

    /// Push binary into a shell command's stdin.
    pub fn stream_in(&mut self, cmd: &str, data: &[u8]) -> Result<(), String> {
        if !self.buf.is_empty() {
            return Err(format!(
                "shell buffer not empty before stream_in: {:?}",
                &self.buf[..self.buf.len().min(64)]
            ));
        }
        let line = format!("({cmd}); echo {SENTINEL}:$?\n");
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| format!("write cmd: {e}"))?;
        self.stream.flush().ok();
        std::thread::sleep(Duration::from_millis(100));
        self.stream
            .write_all(data)
            .map_err(|e| format!("write data: {e}"))?;

        let mut tmp = [0u8; 4096];
        loop {
            if let Some(idx) = self.buf.find(SENTINEL) {
                let before = self.buf[..idx].trim_end().to_string();
                let after = self.buf[idx + SENTINEL.len() + 1..].to_string();
                let (rc_str, rest) = after.split_once('\n').unwrap_or((&after, ""));
                let rc: i32 = rc_str.trim().parse().unwrap_or(-1);
                self.buf = rest.to_string();
                if rc != 0 {
                    return Err(format!("`{cmd}` exit {rc}: {before}"));
                }
                return Ok(());
            }
            let n = self.stream.read(&mut tmp).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("shell closed".into());
            }
            self.buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
    }

    fn await_sentinel(&mut self) -> Result<String, String> {
        let mut tmp = [0u8; 4096];
        loop {
            if let Some(idx) = self.buf.find(SENTINEL) {
                let before = self.buf[..idx].trim_end().to_string();
                let after = self.buf[idx + SENTINEL.len() + 1..].to_string();
                let (_, rest) = after.split_once('\n').unwrap_or((&after, ""));
                self.buf = rest.to_string();
                return Ok(before);
            }
            let n = self.stream.read(&mut tmp).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("shell closed".into());
            }
            self.buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
    }
}
