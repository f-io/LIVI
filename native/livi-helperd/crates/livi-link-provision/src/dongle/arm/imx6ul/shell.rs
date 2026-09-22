//! Root shell on the dongle: busybox telnetd on :2323, one connection per command.
//!
//! The shell has no usable prompt, so the command is wrapped in markers and everything between
//! them is the output. Telnet option negotiation is answered with a refusal (WONT/DONT)

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const DEFAULT_HOST: &str = "10.10.10.1";
pub const TELNET_PORT: u16 = 2323;
pub const PUSH_PORT: u16 = 5610;

const BEGIN: &str = "__LIVI_B__";
const END: &str = "__LIVI_E__";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_SLICE: Duration = Duration::from_millis(500);

pub struct Shell {
    host: String,
}

impl Shell {
    pub fn new(host: &str) -> Self {
        Self { host: host.to_string() }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Resolves "host:port" once, for the callers that open their own sockets.
    pub fn socket_addr(&self, port: u16) -> Result<SocketAddr, String> {
        (self.host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("resolve {}:{port}: {e}", self.host))?
            .next()
            .ok_or_else(|| format!("{}:{port} resolved to nothing", self.host))
    }

    /// Runs one command and returns its output (stdout and stderr).
    pub fn run(&self, cmd: &str, timeout: Duration) -> Result<String, String> {
        let mut s = TcpStream::connect_timeout(&self.socket_addr(TELNET_PORT)?, CONNECT_TIMEOUT)
            .map_err(|e| format!("connect {}:{TELNET_PORT}: {e}", self.host))?;
        s.set_read_timeout(Some(READ_SLICE)).map_err(|e| e.to_string())?;

        sleep(Duration::from_millis(300));
        let mut buf = [0u8; 65536];
        if let Ok(n) = s.read(&mut buf) {
            let (_, refuse) = strip_iac(&buf[..n]);
            if !refuse.is_empty() {
                let _ = s.write_all(&refuse);
            }
        }
        sleep(Duration::from_millis(200));

        // No echo, no PS1/PS2, and a newline before the end marker (output may lack one), so both
        // markers land on lines of their own.
        let script = format!(
            "stty -echo 2>/dev/null; PS1=; PS2=\necho {BEGIN}\n{{\n{cmd}\n}} 2>&1\nprintf '\\n%s\\n' {END}\n"
        );
        s.write_all(script.as_bytes()).map_err(|e| format!("send command: {e}"))?;

        let deadline = Instant::now() + timeout;
        let mut acc: Vec<u8> = Vec::new();
        let (mut complete, mut closed) = (false, false);
        while Instant::now() < deadline {
            match s.read(&mut buf) {
                Ok(0) => closed = true,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
                Err(e) if would_block(&e) => {}
                Err(e) => return Err(format!("read: {e}")),
            }
            if between(&lines(&acc)).is_some() {
                complete = true;
                break;
            }
            if closed {
                break;
            }
        }
        let _ = s.write_all(b"exit\n");
        let _ = s.shutdown(Shutdown::Both);
        if !complete {
            let head: String = cmd.chars().take(80).collect();
            return Err(if closed {
                format!("dongle closed the shell before the command finished: {head}")
            } else {
                format!("dongle command timed out ({timeout:?}): {head}")
            });
        }
        Ok(between(&lines(&acc)).unwrap_or_default())
    }

    /// Convenience for the many short commands, 15s like the rest of the shell work.
    pub fn sh(&self, cmd: &str) -> Result<String, String> {
        self.run(cmd, Duration::from_secs(15))
    }

    /// md5 of a file on the device, `None` if it does not exist.
    pub fn md5(&self, path: &str) -> Option<String> {
        let out = self.sh(&format!("md5sum {path} 2>/dev/null | cut -d' ' -f1")).ok()?;
        let out = out.trim();
        (out.len() >= 32).then(|| out[out.len() - 32..].to_string())
    }

    /// Which of `paths` exist on the device, in the order given.
    pub fn exists(&self, paths: &[String]) -> Result<Vec<String>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let out = self.sh(&format!(
            "for p in {}; do [ -e \"$p\" ] && echo \"$p\"; done",
            paths.join(" ")
        ))?;
        let found: Vec<&str> = out.split_whitespace().collect();
        Ok(paths.iter().filter(|p| found.contains(&p.as_str())).cloned().collect())
    }

    /// Byte size per path, for the paths `ls` can see.
    pub fn sizes(&self, paths: &[String]) -> Result<Vec<(String, u64)>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let out = self.sh(&format!(
            "ls -la {} 2>/dev/null | awk '{{print $5, $NF}}'",
            paths.join(" ")
        ))?;
        let mut sizes = Vec::new();
        for line in out.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(size), Some(path), None) = (parts.next(), parts.next(), parts.next())
                && let Ok(size) = size.parse::<u64>()
            {
                sizes.push((path.to_string(), size));
            }
        }
        Ok(sizes)
    }

    /// Free space on the rootfs in KiB, `None` if `df` did not answer as expected.
    pub fn df_avail_k(&self) -> Result<Option<u64>, String> {
        let out = self.sh("df -k / | tail -1")?;
        Ok(out.split_whitespace().nth(3).and_then(|v| v.parse().ok()))
    }

    /// Whether something accepts connections on that port.
    pub fn port_open(&self, port: u16) -> bool {
        self.socket_addr(port)
            .ok()
            .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok())
            .is_some()
    }

    /// Sends bytes to a `nc` listener into /tmp, verifies them there, copies onto flash and
    /// renames — so a half-written file never carries the final name.
    pub fn push(&self, data: &[u8], remote: &str, port: u16, want: &str) -> Result<(), String> {
        let tmp = format!("/tmp/livi-push.{port}");
        let mut last = String::new();
        for attempt in 1..=3 {
            self.sh(&format!(
                "pkill -f 'nc -l -p {port}' 2>/dev/null; rm -f {tmp}; \
                 setsid sh -c 'nc -l -p {port} > {tmp}' >/dev/null 2>&1 &"
            ))?;
            sleep(Duration::from_secs(1));

            match self.send(data, port) {
                Ok(()) => {}
                Err(e) => {
                    last = format!("attempt {attempt}: {e}");
                    sleep(Duration::from_secs(2));
                    continue;
                }
            }
            // Give the device time to flush what it received before hashing it.
            sleep(Duration::from_millis(500) + Duration::from_secs_f64(data.len() as f64 / 4e6));
            if self.md5(&tmp).as_deref() != Some(want) {
                last = format!("attempt {attempt}: md5 mismatch in /tmp");
                continue;
            }

            self.run(
                &format!("mkdir -p $(dirname {remote}) && cp {tmp} {remote}.tmp && sync"),
                Duration::from_secs(90),
            )?;
            if self.md5(&format!("{remote}.tmp")).as_deref() != Some(want) {
                return Err(format!("flash copy of {remote} failed md5"));
            }
            self.run(
                &format!("mv {remote}.tmp {remote} && sync && rm -f {tmp}"),
                Duration::from_secs(30),
            )?;
            return Ok(());
        }
        Err(format!("push to {remote} failed after 3 attempts ({last})"))
    }

    fn send(&self, data: &[u8], port: u16) -> Result<(), String> {
        let mut c = TcpStream::connect_timeout(&self.socket_addr(port)?, Duration::from_secs(8))
            .map_err(|e| format!("connect: {e}"))?;
        c.write_all(data).map_err(|e| format!("send: {e}"))?;
        c.shutdown(Shutdown::Write).map_err(|e| format!("shutdown: {e}"))?;
        Ok(())
    }
}

fn would_block(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

/// Strips telnet control sequences, returning the payload and the refusal to send back:
/// DO -> WONT, WILL -> DONT. NUL bytes (telnet line endings) are dropped.
fn strip_iac(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut refuse) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 255 && i + 2 < data.len() {
            match data[i + 1] {
                253 => refuse.extend_from_slice(&[255, 252, data[i + 2]]),
                251 => refuse.extend_from_slice(&[255, 254, data[i + 2]]),
                _ => {}
            }
            i += 3;
            continue;
        }
        if b != 0 {
            out.push(b);
        }
        i += 1;
    }
    (out, refuse)
}

fn lines(buf: &[u8]) -> Vec<String> {
    let (clean, _) = strip_iac(buf);
    String::from_utf8_lossy(&clean)
        .replace('\r', "")
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// The lines between the markers, once both have arrived.
fn between(lines: &[String]) -> Option<String> {
    let begin = lines.iter().position(|l| l == BEGIN)?;
    let end = begin + 1 + lines[begin + 1..].iter().position(|l| l == END)?;
    Some(lines[begin + 1..end].join("\n").trim_matches('\n').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_telnet_options_and_drops_nuls() {
        let (out, refuse) = strip_iac(&[255, 253, 24, b'h', 0, b'i', 255, 251, 1]);
        assert_eq!(out, b"hi");
        assert_eq!(refuse, vec![255, 252, 24, 255, 254, 1]);
    }

    #[test]
    fn takes_the_lines_between_the_markers() {
        let buf = format!("junk\r\n{BEGIN}\r\nout1\r\nout2\r\n{END}\r\ntrailer").into_bytes();
        assert_eq!(between(&lines(&buf)).as_deref(), Some("out1\nout2"));
    }

    #[test]
    fn incomplete_output_is_not_a_result() {
        let buf = format!("{BEGIN}\r\nout1\r\n").into_bytes();
        assert!(between(&lines(&buf)).is_none());
    }

    #[test]
    fn an_end_marker_before_the_begin_marker_does_not_count() {
        let buf = format!("{END}\r\n{BEGIN}\r\nout\r\n{END}\r\n").into_bytes();
        assert_eq!(between(&lines(&buf)).as_deref(), Some("out"));
    }
}
