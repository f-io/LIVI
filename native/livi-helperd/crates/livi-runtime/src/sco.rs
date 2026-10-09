// Call audio bridge, CVSD = raw PCM s16le 8kHz. On Linux it accepts the phone's SCO connection
// itself, on the Mac our own host hands it over as a datagram socket. The caller's samples go
// straight into the pipeline's feed as audio records, the microphone comes back from the
// pipeline's tap over MIC_SOCK. The main process only says which feed and stream id to use.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use livi_host_proto::feed::{self as feedproto, Framer};

/// Where the main process's microphone tap connects for a call.
pub const MIC_SOCK: &str = "/tmp/aa-sco.mic";
/// Microphone samples waiting for their SCO frame, beyond that the oldest go.
const MIC_BACKLOG: usize = 8000 * 2;

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

pub fn mic_socket() -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(MIC_SOCK);
    let mic = UnixListener::bind(MIC_SOCK)?;
    std::fs::set_permissions(MIC_SOCK, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
    mic.set_nonblocking(true)?;
    Ok(mic)
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

struct Call<'a> {
    sink: &'a ScoSink,
    mic: &'a UnixListener,
    downlink: Option<Downlink>,
    tap: Option<UnixStream>,
    framer: Framer,
    pending: VecDeque<u8>,
    chunk: Vec<u8>,
}

impl<'a> Call<'a> {
    fn new(sink: &'a ScoSink, mic: &'a UnixListener) -> Self {
        Self {
            sink,
            mic,
            downlink: None,
            tap: None,
            framer: Framer::new(),
            pending: VecDeque::new(),
            chunk: vec![0u8; 4096],
        }
    }

    /// The caller into the feed, reconnecting when the target changes.
    fn caller(&mut self, pcm: &[u8]) {
        let target = self.sink.get();
        if self.downlink.as_ref().is_some_and(|d| Some(&d.target) != target.as_ref()) {
            self.downlink = None;
        }
        if self.downlink.is_none()
            && let Some(t) = target
        {
            self.downlink = open_downlink(t);
        }
        if let Some(d) = self.downlink.as_mut() {
            let record = feedproto::encode(feedproto::KIND_AUDIO, d.target.1, now_ns(), pcm);
            if d.sock.write_all(&record).is_err() {
                eprintln!("[sco] feed gone");
                self.downlink = None;
            }
        }
    }

    /// Whatever the tap delivered since the last time.
    fn listen(&mut self) {
        if let Ok((s, _)) = self.mic.accept() {
            s.set_nonblocking(true).ok();
            println!("[sco] microphone tap attached");
            self.tap = Some(s);
            self.framer = Framer::new();
            self.pending.clear();
        }
        let Some(t) = self.tap.as_mut() else { return };
        match t.read(&mut self.chunk) {
            Ok(0) => {
                println!("[sco] microphone tap detached");
                self.tap = None;
            }
            Ok(read) => {
                self.framer.push(&self.chunk[..read]);
                while let Some(r) = self.framer.next_record() {
                    if r.kind == feedproto::KIND_MIC {
                        self.pending.extend(r.payload);
                    }
                }
                while self.pending.len() > MIC_BACKLOG {
                    self.pending.pop_front();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
                println!("[sco] microphone tap detached");
                self.tap = None;
            }
        }
    }

    /// Zeros where the microphone has nothing yet.
    fn fill(&mut self, up: &mut [u8]) {
        for byte in up {
            *byte = self.pending.pop_front().unwrap_or(0);
        }
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn announce(events: &crate::livi_sock::Broadcaster, up: bool, mtu: usize) {
    if up {
        println!("[sco] audio connected (mtu {mtu})");
        events.push_json(format!("{{\"event\":\"sco\",\"up\":true,\"mtu\":{mtu}}}"));
    } else {
        println!("[sco] audio closed");
        events.push_json("{\"event\":\"sco\",\"up\":false}".to_string());
    }
}

/// One call from its first packet to its last.
pub fn call(
    events: &crate::livi_sock::Broadcaster,
    link: &livi_hfp::sco::Link,
    mic: &UnixListener,
    sink: &ScoSink,
) {
    announce(events, true, link.mtu);
    bridge(link, mic, sink);
    announce(events, false, link.mtu);
}

/// One frame down, one frame up per cycle, the SCO read paces the uplink.
fn bridge(link: &livi_hfp::sco::Link, mic: &UnixListener, sink: &ScoSink) {
    let mut call = Call::new(sink, mic);
    let mut down = [0u8; livi_hfp::sco::MAX_PACKET];
    let mut up = [0u8; livi_hfp::sco::MAX_PACKET];
    while let Ok(n @ 1..) = link.read(&mut down) {
        call.caller(&down[..n]);
        call.listen();
        call.fill(&mut up[..n]);
        link.send(&up[..n]);
    }
}

#[cfg(target_os = "linux")]
pub fn serve(events: crate::livi_sock::Broadcaster, sink: ScoSink) {
    std::thread::spawn(move || linux::run(events, sink));
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ScoSink, mic_socket};

    pub fn run(events: crate::livi_sock::Broadcaster, sink: ScoSink) {
        let mic = match mic_socket() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[sco] mic socket failed: {e}");
                return;
            }
        };
        let listen = match livi_hfp::sco::listen() {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("[sco] listen failed: {e}");
                return;
            }
        };
        println!("[sco] listening (SCO + {})", super::MIC_SOCK);

        loop {
            let link = match livi_hfp::sco::accept(&listen) {
                Ok(link) => link,
                Err(e) => {
                    eprintln!("[sco] accept failed: {e}");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            };
            super::call(&events, &link, &mic, &sink);
        }
    }
}
