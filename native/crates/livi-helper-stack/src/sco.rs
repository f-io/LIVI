// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

use livi_core_proto::aa_events::AaHelperEvent;
use livi_output_proto::feed::{self as feedproto, Framer};

const MIC_BACKLOG: usize = 8000 * 2;

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

pub fn mic_socket(path: &Path) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let mic = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
    mic.set_nonblocking(true)?;
    Ok(mic)
}

struct Downlink {
    target: (String, u32),
    sock: UnixStream,
}

fn open_downlink(target: (String, u32)) -> Option<Downlink> {
    match UnixStream::connect(&target.0) {
        Ok(sock) => {
            println!(
                "[sco] call audio goes to the feed as stream 0x{:x}",
                target.1
            );
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

    fn caller(&mut self, pcm: &[u8]) {
        let target = self.sink.get();
        if self
            .downlink
            .as_ref()
            .is_some_and(|d| Some(&d.target) != target.as_ref())
        {
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
                    let Ok(r) = r else {
                        println!("[sco] microphone tap detached");
                        self.tap = None;
                        break;
                    };
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
        events.push_json(
            AaHelperEvent::Sco {
                up: true,
                mtu: Some(mtu),
            }
            .to_line(),
        );
    } else {
        println!("[sco] audio closed");
        events.push_json(
            AaHelperEvent::Sco {
                up: false,
                mtu: None,
            }
            .to_line(),
        );
    }
}

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
pub fn serve(mic: std::path::PathBuf, events: crate::livi_sock::Broadcaster, sink: ScoSink) {
    std::thread::spawn(move || linux::run(&mic, events, sink));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;

    use super::{ScoSink, mic_socket};

    pub fn run(path: &Path, events: crate::livi_sock::Broadcaster, sink: ScoSink) {
        let mic = match mic_socket(path) {
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
        println!("[sco] listening (SCO + {})", path.display());

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
