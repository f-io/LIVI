use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Hfp {
    inner: Arc<HfpInner>,
}

#[derive(Default)]
struct HfpInner {
    established: AtomicBool,
    events: Mutex<Option<crate::livi_sock::Broadcaster>>,
    links: Mutex<HashSet<String>>,
    /// Phones ringing, dialling or in a call.
    calls: AtomicUsize,
}

impl Hfp {
    pub fn established(&self) -> bool {
        self.inner.established.load(Ordering::SeqCst)
    }

    pub fn set_events(&self, events: crate::livi_sock::Broadcaster) {
        *self.inner.events.lock().unwrap() = Some(events);
    }

    pub fn linked(&self, mac: &str) -> bool {
        self.inner.links.lock().unwrap().contains(&mac.to_uppercase())
    }

    pub fn in_call(&self) -> bool {
        self.inner.calls.load(Ordering::Relaxed) > 0
    }

    pub fn accept(&self, fd: std::os::fd::OwnedFd, mac: String) {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || slc::slc_loop(&inner, fd, Vec::new(), true, &mac));
    }
}

mod slc {
    use super::HfpInner;
    use livi_hfp::Slc;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    const AT_TIMEOUT: Duration = Duration::from_secs(300);

    /// AT_TIMEOUT applies only while the SLC is negotiating, an established one may stay silent.
    pub fn slc_loop(inner: &HfpInner, fd: OwnedFd, initial: Vec<u8>, send_hello: bool, mac: &str) {
        let mut slc = Slc::default();
        if send_hello && write_all(&fd, Slc::hello().as_bytes()).is_err() {
            return;
        }
        inner.links.lock().unwrap().insert(mac.to_uppercase());
        let mut calling = false;
        let mut was_up = false;
        let mut last_batt: Option<u8> = None;
        let mut buf = initial;
        loop {
            while let Some(pos) = buf.iter().position(|b| *b == b'\r') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
                for out in slc.on_line(&line) {
                    if write_all(&fd, out.as_bytes()).is_err() {
                        return finish(inner, mac, was_up, calling);
                    }
                }
                if slc.in_call() != calling {
                    calling = !calling;
                    if calling {
                        inner.calls.fetch_add(1, Ordering::Relaxed);
                    } else {
                        inner.calls.fetch_sub(1, Ordering::Relaxed);
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
                        emit(
                            inner,
                            &format!(
                                "{{\"event\":\"phone-battery\",\"mac\":\"{mac}\",\"pct\":{pct}}}"
                            ),
                        );
                    }
                }
            }
            while !readable(&fd, AT_TIMEOUT) {
                if hung_up(&fd) {
                    println!("[hfp] peer closed");
                    return finish(inner, mac, was_up, calling);
                }
                if !slc.established() {
                    println!("[hfp] AT timeout during SLC setup — disconnecting");
                    return finish(inner, mac, was_up, calling);
                }
            }
            let mut chunk = [0u8; 1024];
            let n = unsafe { libc::read(fd.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                println!("[hfp] disconnected");
                return finish(inner, mac, was_up, calling);
            }
            buf.extend_from_slice(&chunk[..n as usize]);
        }
    }

    fn emit(inner: &HfpInner, line: &str) {
        if let Some(ev) = inner.events.lock().unwrap().as_ref() {
            ev.push_json(line.to_string());
        }
    }

    fn finish(inner: &HfpInner, mac: &str, was_up: bool, calling: bool) {
        inner.links.lock().unwrap().remove(&mac.to_uppercase());
        if calling {
            inner.calls.fetch_sub(1, Ordering::Relaxed);
        }
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
