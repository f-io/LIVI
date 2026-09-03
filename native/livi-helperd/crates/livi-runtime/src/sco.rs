// SCO audio bridge: accepts the phone's SCO connection (CVSD = raw PCM s16le 8kHz).
// The caller's samples go straight into the pipeline's feed as audio records, the
// microphone comes back from the pipeline's tap over MIC_SOCK. One frame in, one
// frame out keeps the air pacing. The main process only says which feed and
// stream id to use.

use std::sync::{Arc, Mutex};

/// Where the main process's microphone tap connects for a call.
pub const MIC_SOCK: &str = "/tmp/aa-sco.mic";

/// The feed path and stream id the call audio goes to, set by the main process.
#[derive(Clone, Default)]
pub struct ScoSink(Arc<Mutex<Option<(String, u32)>>>);

impl ScoSink {
    pub fn set(&self, target: Option<(String, u32)>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = target;
    }

    fn get(&self) -> Option<(String, u32)> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(target_os = "linux")]
pub fn serve(events: crate::livi_sock::Broadcaster, sink: ScoSink) {
    std::thread::spawn(move || linux::run(events, sink));
}

#[cfg(not(target_os = "linux"))]
pub fn serve(_events: crate::livi_sock::Broadcaster, _sink: ScoSink) {}

#[cfg(target_os = "linux")]
mod linux {
    use super::{MIC_SOCK, ScoSink};
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::{UnixListener, UnixStream};

    use livi_host_proto::feed::{self as feedproto, Framer};

    const BTPROTO_SCO: libc::c_int = 2;
    const SOL_SCO: libc::c_int = 17;
    const SCO_OPTIONS: libc::c_int = 1;
    /// Microphone samples waiting for their SCO frame, beyond that the oldest go.
    const MIC_BACKLOG: usize = 8000 * 2;

    #[repr(C)]
    struct SockaddrSco {
        sco_family: libc::sa_family_t,
        sco_bdaddr: [u8; 6],
    }

    #[repr(C)]
    struct ScoOptions {
        mtu: u16,
    }

    pub fn run(events: crate::livi_sock::Broadcaster, sink: ScoSink) {
        let _ = std::fs::remove_file(MIC_SOCK);
        let mic = match UnixListener::bind(MIC_SOCK) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[sco] mic socket failed: {e}");
                return;
            }
        };
        let _ = std::fs::set_permissions(
            MIC_SOCK,
            std::os::unix::fs::PermissionsExt::from_mode(0o666),
        );
        mic.set_nonblocking(true).ok();

        let listen = match sco_listen() {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("[sco] listen failed: {e}");
                return;
            }
        };
        println!("[sco] listening (SCO + {MIC_SOCK})");

        loop {
            let sco = match sco_accept(&listen) {
                Ok(pair) => pair,
                Err(e) => {
                    eprintln!("[sco] accept failed: {e}");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            };
            let (sco_fd, mtu) = sco;
            println!("[sco] audio connected (mtu {mtu})");
            events.push_json(format!("{{\"event\":\"sco\",\"up\":true,\"mtu\":{mtu}}}"));
            bridge(&sco_fd, mtu as usize, &mic, &sink);
            println!("[sco] audio closed");
            events.push_json("{\"event\":\"sco\",\"up\":false}".to_string());
        }
    }

    /// The feed connection for the caller's samples, opened once the target is known.
    struct Downlink {
        target: (String, u32),
        sock: UnixStream,
    }

    fn open_downlink(target: (String, u32)) -> Option<Downlink> {
        match UnixStream::connect(&target.0) {
            Ok(sock) => {
                println!("[sco] call audio goes to the feed as stream 0x{:x}", target.1);
                Some(Downlink { target, sock })
            }
            Err(e) => {
                eprintln!("[sco] feed {}: {e}", target.0);
                None
            }
        }
    }

    /// One frame down, one frame up per cycle, the SCO read paces the uplink.
    fn bridge(sco: &OwnedFd, mtu: usize, mic: &UnixListener, sink: &ScoSink) {
        let mut down = vec![0u8; mtu.max(48)];
        let mut up = vec![0u8; mtu.max(48)];
        let mut downlink: Option<Downlink> = None;
        let mut tap: Option<UnixStream> = None;
        let mut framer = Framer::new();
        let mut pending: VecDeque<u8> = VecDeque::new();
        let mut chunk = vec![0u8; 4096];
        loop {
            if let Ok((s, _)) = mic.accept() {
                s.set_nonblocking(true).ok();
                println!("[sco] microphone tap attached");
                tap = Some(s);
                framer = Framer::new();
                pending.clear();
            }
            let n = unsafe { libc::read(sco.as_raw_fd(), down.as_mut_ptr().cast(), down.len()) };
            if n <= 0 {
                return;
            }
            let n = n as usize;

            // Down: the caller into the feed, reconnecting when the target changes.
            let target = sink.get();
            if downlink.as_ref().is_some_and(|d| Some(&d.target) != target.as_ref()) {
                downlink = None;
            }
            if downlink.is_none()
                && let Some(t) = target
            {
                downlink = open_downlink(t);
            }
            if let Some(d) = downlink.as_mut() {
                let record = feedproto::encode(feedproto::KIND_AUDIO, d.target.1, now_ns(), &down[..n]);
                if d.sock.write_all(&record).is_err() {
                    eprintln!("[sco] feed gone");
                    downlink = None;
                }
            }

            // Up: whatever the tap delivered since the last frame, zeros when nothing.
            if let Some(t) = tap.as_mut() {
                match t.read(&mut chunk) {
                    Ok(0) => {
                        println!("[sco] microphone tap detached");
                        tap = None;
                    }
                    Ok(read) => {
                        framer.push(&chunk[..read]);
                        while let Some(r) = framer.next_record() {
                            if r.kind == feedproto::KIND_MIC {
                                pending.extend(r.payload);
                            }
                        }
                        while pending.len() > MIC_BACKLOG {
                            pending.pop_front();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => {
                        println!("[sco] microphone tap detached");
                        tap = None;
                    }
                }
            }
            for byte in up[..n].iter_mut() {
                *byte = pending.pop_front().unwrap_or(0);
            }
            let _ = unsafe { libc::write(sco.as_raw_fd(), up.as_ptr().cast(), n) };
        }
    }

    fn now_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn sco_listen() -> std::io::Result<OwnedFd> {
        let raw = unsafe { libc::socket(libc::AF_BLUETOOTH, libc::SOCK_SEQPACKET, BTPROTO_SCO) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let addr = SockaddrSco {
            sco_family: libc::AF_BLUETOOTH as libc::sa_family_t,
            sco_bdaddr: [0; 6], // BDADDR_ANY
        };
        let rc = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                std::ptr::addr_of!(addr).cast(),
                std::mem::size_of::<SockaddrSco>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::listen(fd.as_raw_fd(), 1) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(fd)
    }

    fn sco_accept(listen: &OwnedFd) -> std::io::Result<(OwnedFd, u16)> {
        let raw = unsafe { libc::accept(listen.as_raw_fd(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut opts = ScoOptions { mtu: 48 };
        let mut len = std::mem::size_of::<ScoOptions>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                SOL_SCO,
                SCO_OPTIONS,
                std::ptr::addr_of_mut!(opts).cast(),
                &mut len,
            );
        }
        Ok((fd, opts.mtu.max(24)))
    }
}
