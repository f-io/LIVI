// HFP Hands-Free: SLC over RFCOMM so the phone treats the head unit as a car kit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// Bit 2 CLI, bit 3 voice recognition, bit 4 remote volume. No codec negotiation:
// the AG then defaults to CVSD and SCO carries raw PCM s16le 8kHz.
pub const HF_FEATURES: u32 = (1 << 2) | (1 << 3) | (1 << 4);

const OK: &str = "\r\nOK\r";

/// SLC engine: AG lines in, HF lines out. HF speaks first with AT+BRSF; the OK chain
/// walks BRSF → (BAC) → CIND=? → CIND? → CMER, after which the SLC stands.
#[derive(Default)]
pub struct Slc {
    ag_features: u32,
    sent_bac: bool,
    sent_cind_test: bool,
    sent_cind_read: bool,
    sent_cmer: bool,
    established: bool,
    post: Vec<&'static str>,
    indicators: Vec<String>,
    /// battchg 0-5, updated by +CIND? and +CIEV.
    pub battchg: Option<u8>,
}

impl Slc {
    pub fn established(&self) -> bool {
        self.established
    }

    /// The opening command when no probe has sent it yet.
    pub fn hello() -> String {
        format!("AT+BRSF={HF_FEATURES}\r")
    }

    pub fn on_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        println!("[hfp] << {line}");

        if let Some(v) = line.strip_prefix("+BRSF:") {
            self.ag_features = v.trim().parse().unwrap_or(self.ag_features);
            return vec![];
        }
        if line == "OK" {
            if self.established {
                if !self.post.is_empty() {
                    return vec![self.post.remove(0).into()];
                }
                return vec![];
            }
            let both_codec_neg =
                HF_FEATURES & (1 << 7) != 0 && self.ag_features & (1 << 9) != 0;
            if self.ag_features > 0 && both_codec_neg && !self.sent_bac {
                self.sent_bac = true;
                return vec!["AT+BAC=1,2\r".into()];
            }
            if self.ag_features > 0 && !self.sent_cind_test {
                self.sent_cind_test = true;
                return vec!["AT+CIND=?\r".into()];
            }
            if self.sent_cind_test && !self.sent_cind_read {
                self.sent_cind_read = true;
                return vec!["AT+CIND?\r".into()];
            }
            if self.sent_cind_read && !self.sent_cmer {
                self.sent_cmer = true;
                return vec!["AT+CMER=3,0,0,1\r".into()];
            }
            if self.sent_cmer && !self.established {
                self.established = true;
                println!("[hfp] SLC established");
                // Android drops a silent HF after ~12s; the post-SLC dialogue
                // (same sequence PipeWire used) keeps the link alive.
                self.post = vec![
                    "AT+CLIP=1\r",
                    "AT+CCWA=1\r",
                    "AT+CMEE=1\r",
                    "AT+CLCC\r",
                    "AT+VGS=12\r",
                    "AT+VGM=12\r",
                ];
                return vec![self.post.remove(0).into()];
            }
            return vec![];
        }
        if let Some(v) = line.strip_prefix("+CIND:") {
            let v = v.trim();
            if v.starts_with('(') {
                // Test response: ("call",(0,1)),... — capture the order.
                self.indicators = v
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect();
            } else {
                // Read response: current values in the captured order.
                for (i, val) in v.split(',').enumerate() {
                    if self.indicators.get(i).map(String::as_str) == Some("battchg") {
                        self.battchg = val.trim().parse().ok();
                    }
                }
            }
            return vec![];
        }
        if line == "ERROR" {
            return vec![];
        }

        if let Some(v) = line.strip_prefix("AT+BRSF=") {
            self.ag_features = v.trim().parse().unwrap_or(self.ag_features);
            return vec![format!("\r\n+BRSF: {HF_FEATURES}"), OK.into()];
        }
        if line == "AT+CIND=?" {
            return vec![
                "\r\n+CIND: (\"service\",(0,1)),(\"call\",(0,1)),(\"callsetup\",(0-3)),(\"callheld\",(0-2)),(\"signal\",(0-5)),(\"roam\",(0,1)),(\"battchg\",(0-5))".into(),
                OK.into(),
            ];
        }
        if line == "AT+CIND?" {
            return vec!["\r\n+CIND: 1,0,0,0,5,0,5".into(), OK.into()];
        }
        if line.starts_with("AT+CHLD=?") {
            return vec!["\r\n+CHLD: (0,1,2,3)".into(), OK.into()];
        }
        if line.starts_with("AT+BIND=?") {
            return vec!["\r\n+BIND: (1,2)".into(), OK.into()];
        }
        if line.starts_with("AT+BIND?") {
            return vec!["\r\n+BIND: 1,1".into(), "\r\n+BIND: 2,1".into(), OK.into()];
        }
        if let Some(v) = line.strip_prefix("+BCS:") {
            return vec![format!("AT+BCS={}\r", v.trim())];
        }
        if line.starts_with("AT+COPS") {
            if line.contains("=?") {
                return vec![OK.into()];
            }
            if line.contains('?') {
                return vec!["\r\n+COPS: 0,0,\"Carrier\"".into(), OK.into()];
            }
            return vec![OK.into()];
        }
        if let Some(v) = line.strip_prefix("+CIEV:") {
            let mut it = v.trim().split(',');
            let idx: usize = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            let val: u8 = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            // +CIEV indices are 1-based over the +CIND=? order.
            if idx >= 1 && self.indicators.get(idx - 1).map(String::as_str) == Some("battchg") {
                self.battchg = Some(val);
            }
            return vec![];
        }
        if line == "RING" || line.starts_with("+CLIP:") {
            return vec![];
        }
        // Everything else the phone asks gets acknowledged: AT+CMER=, AT+BIND=, AT+BAC=,
        // ATA, AT+CHUP, ATD…, AT+BVRA=, AT+VGS=, AT+VGM=, AT+NREC=, AT+BTRH?, AT+CLIP=,
        // AT+CCWA=, AT+CMEE=, AT+CLCC, AT+CNUM, unknown.
        vec![OK.into()]
    }
}

/// Trigger + cooldown + per-phone channel cache around the raw RFCOMM prober.
#[derive(Clone, Default)]
pub struct Hfp {
    inner: Arc<HfpInner>,
}

#[derive(Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct HfpInner {
    established: AtomicBool,
    owned_elsewhere: AtomicBool,
    events: Mutex<Option<crate::livi_sock::Broadcaster>>,
}

impl Hfp {
    pub fn established(&self) -> bool {
        self.inner.established.load(Ordering::SeqCst)
    }

    /// The audio daemon holds the HF profile (incl. SCO)
    pub fn set_owned_elsewhere(&self) {
        self.inner.owned_elsewhere.store(true, Ordering::SeqCst);
    }

    /// Event sink for SLC state and battery updates.
    pub fn set_events(&self, events: crate::livi_sock::Broadcaster) {
        *self.inner.events.lock().unwrap() = Some(events);
    }

    /// Channel probing is retired: LIVI's keeper connects via the registered
    /// profile (ConnectProfile), incoming SLCs land in accept().
    #[cfg(target_os = "linux")]
    pub fn trigger(&self, _mac: &str) {}

    /// Incoming Profile1 connection: the AG connected to us, run the SLC on its fd.
    #[cfg(target_os = "linux")]
    pub fn accept(&self, fd: std::os::fd::OwnedFd, mac: String) {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || linux::slc_loop(&inner, fd, Vec::new(), true, &mac));
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{HfpInner, Slc};
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    const AT_TIMEOUT: Duration = Duration::from_secs(300);

    /// Blocking AT loop until the peer hangs up. AT_TIMEOUT applies only while the SLC
    /// is still negotiating. An established SLC is silent by design.
    pub fn slc_loop(inner: &HfpInner, fd: OwnedFd, initial: Vec<u8>, send_hello: bool, mac: &str) {
        let mut slc = Slc::default();
        if send_hello && write_all(&fd, Slc::hello().as_bytes()).is_err() {
            return;
        }
        let mut was_up = false;
        let mut last_batt: Option<u8> = None;
        let mut buf = initial;
        loop {
            while let Some(pos) = buf.iter().position(|b| *b == b'\r') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
                for out in slc.on_line(&line) {
                    if write_all(&fd, out.as_bytes()).is_err() {
                        return finish(inner, mac, was_up);
                    }
                }
                if slc.established() && !was_up {
                    was_up = true;
                    inner.established.store(true, Ordering::SeqCst);
                    emit(inner, &format!("{{\"event\":\"hfp\",\"up\":true,\"mac\":\"{mac}\"}}"));
                }
                if slc.battchg != last_batt {
                    last_batt = slc.battchg;
                    if let Some(b) = slc.battchg {
                        let pct = u32::from(b.min(5)) * 20;
                        emit(inner, &format!("{{\"event\":\"phone-battery\",\"mac\":\"{mac}\",\"pct\":{pct}}}"));
                    }
                }
            }
            while !readable(&fd, AT_TIMEOUT) {
                if hung_up(&fd) {
                    println!("[hfp] peer closed");
                    return finish(inner, mac, was_up);
                }
                if !slc.established() {
                    println!("[hfp] AT timeout during SLC setup — disconnecting");
                    return finish(inner, mac, was_up);
                }
            }
            let mut chunk = [0u8; 1024];
            let n = unsafe { libc::read(fd.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                println!("[hfp] disconnected");
                return finish(inner, mac, was_up);
            }
            buf.extend_from_slice(&chunk[..n as usize]);
        }
    }

    fn emit(inner: &HfpInner, line: &str) {
        if let Some(ev) = inner.events.lock().unwrap().as_ref() {
            ev.push_json(line.to_string());
        }
    }

    fn finish(inner: &HfpInner, mac: &str, was_up: bool) {
        inner.established.store(false, Ordering::SeqCst);
        if was_up {
            emit(inner, &format!("{{\"event\":\"hfp\",\"up\":false,\"mac\":\"{mac}\"}}"));
        }
    }

    fn poll(fd: &OwnedFd, events: libc::c_short, timeout: Duration) -> bool {
        let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 };
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
        rc > 0 && pfd.revents & events != 0
    }

    fn readable(fd: &OwnedFd, timeout: Duration) -> bool {
        poll(fd, libc::POLLIN, timeout)
    }

    fn hung_up(fd: &OwnedFd) -> bool {
        let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let rc = unsafe { libc::poll(&mut pfd, 1, 0) };
        rc > 0 && pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0
    }

    fn write_all(fd: &OwnedFd, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            let n = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
            if n <= 0 {
                return Err(std::io::Error::last_os_error());
            }
            data = &data[n as usize..];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(slc: &mut Slc, line: &str) -> Vec<String> {
        slc.on_line(line)
    }

    #[test]
    fn slc_walks_to_established_and_tracks_battchg() {
        let mut slc = Slc::default();
        assert_eq!(Slc::hello(), format!("AT+BRSF={HF_FEATURES}\r"));
        assert!(drive(&mut slc, "+BRSF:4095").is_empty());
        // No codec negotiation offered — straight to the indicator dance.
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CIND=?\r"]);
        assert!(drive(&mut slc, "+CIND: (\"call\",(0,1)),(\"battchg\",(0-5))").is_empty());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CIND?\r"]);
        assert!(drive(&mut slc, "+CIND: 0,4").is_empty());
        assert_eq!(slc.battchg, Some(4));
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CMER=3,0,0,1\r"]);
        assert!(!slc.established());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CLIP=1\r"]);
        assert!(slc.established());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CCWA=1\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CMEE=1\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CLCC\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+VGS=12\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+VGM=12\r"]);
        assert!(drive(&mut slc, "OK").is_empty());
        assert!(drive(&mut slc, "+CIEV: 2,3").is_empty());
        assert_eq!(slc.battchg, Some(3));
    }

    #[test]
    fn answers_ag_initiated_commands() {
        let mut slc = Slc::default();
        let out = drive(&mut slc, "AT+BRSF=511");
        assert_eq!(out, vec![format!("\r\n+BRSF: {HF_FEATURES}"), "\r\nOK\r".to_string()]);
        assert_eq!(drive(&mut slc, "AT+CIND?"), vec!["\r\n+CIND: 1,0,0,0,5,0,5", "\r\nOK\r"]);
        assert_eq!(drive(&mut slc, "+BCS:2"), vec!["AT+BCS=2\r"]);
        assert_eq!(drive(&mut slc, "AT+COPS?"), vec!["\r\n+COPS: 0,0,\"Carrier\"", "\r\nOK\r"]);
        assert_eq!(drive(&mut slc, "AT+WEIRD"), vec!["\r\nOK\r"]);
        assert!(drive(&mut slc, "RING").is_empty());
    }
}
