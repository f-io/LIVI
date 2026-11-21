// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The Bluetrum AB5301B runs the bluetooth profiles itself. livi-core owns its UART and passes on:
//! `AT#` text lines out and two-letter replies back as cmd d4, the RFCOMM bytes of a phone's iAP2
//! or android auto channel both ways as cmd d6, at most 240 per frame.
//!
//! Replies: `AD` the phone's address, `MZ` the chip's own, `UU` a service the phone offers, `SV` an
//! iAP2 channel is open, `SW` an android auto one. Once a phone is connected the accessory speaks
//! iAP2 first and the chip opens the RFCOMM channel for it. Some firmware wants `AT#SP<address><uuid>`.
//! Some firmware passes the channel's bytes as hex lines instead, `SI` in and `AT#SG` out.
//! `AT#CD` drops the connected phone, as the stock firmware does once it has moved on.
//!
//! The chip holds one phone at a time: `IB` and `AD` bring it, `IA` takes it away. It reconnects the
//! phone it held last on its own, before LIVI's access point is up. `AT#CD` also stops that until the
//! chip restarts, so LIVI drops it at boot and pages with `AT#CC` (no address, the phone it held last)
//! once the access point is up and the CPU has room for the session.
//!
//! The chip has one channel and does not say whose it is, so a phone that leaves iAP2 unanswered
//! gets it as android auto.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use livi_core_proto::helper::MODULE_BT_MAC;
use livi_helper_stack::hfp::Hfp;
use livi_helper_stack::reconnect::{Link, Paging};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use crate::bt_dongle::Arrival;

const HOST: u8 = 0x80;
const CHIP: u8 = 0x81;
const LINE: u8 = 0xd4;
const DATA: u8 = 0xd6;
const CHUNK: usize = 240;
const HEX_CHUNK: usize = 100;
const IAP2: &str = "00000000DECAFADEDECADEAFDECACAFE";
const RETRY: Duration = Duration::from_secs(5);
/// An iPhone answers iAP2 within a second or two.
const IAP2_ANSWER: Duration = Duration::from_secs(8);
const TICK: Duration = Duration::from_secs(1);
const PAGE_WAIT: Duration = Duration::from_secs(8);
/// One-minute load under which the single core takes a session's start.
const CALM_LOAD: f32 = 1.5;
const CALM_WAIT: Duration = Duration::from_secs(60);

static PORT: Mutex<Option<Chip>> = Mutex::new(None);
/// The bool: its iAP2 channel was asked for again since.
static HELD: Mutex<Option<(String, bool)>> = Mutex::new(None);

#[derive(Clone)]
pub struct Profiles {
    pub carplay: Option<UnboundedSender<Arrival>>,
    pub android_auto: Option<UnboundedSender<Arrival>>,
    pub hfp: Hfp,
}

fn frame(cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x02, HOST, cmd, data.len() as u8];
    f.extend_from_slice(data);
    let xor = f[1..].iter().fold(0, |x, b| x ^ b);
    f.push(xor);
    f
}

fn next_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let start = buf.iter().position(|&b| b == 0x02).unwrap_or(buf.len());
        buf.drain(..start);
        if buf.len() < 5 || buf.len() < 5 + usize::from(buf[3]) {
            return None;
        }
        let len = 5 + usize::from(buf[3]);
        if buf[1..len - 1].iter().fold(0, |x, b| x ^ b) == buf[len - 1] {
            return Some(buf.drain(..len).collect());
        }
        buf.remove(0);
    }
}

fn address(hex: &str) -> Option<[u8; 6]> {
    let hex = hex.get(hex.len().checked_sub(12)?..)?;
    let mut out = [0u8; 6];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn text(addr: &[u8; 6]) -> String {
    addr.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[derive(Clone)]
struct Chip(Arc<Mutex<UnixStream>>);

impl Chip {
    fn send(&self, cmd: u8, data: &[u8]) -> bool {
        self.0.lock().unwrap().write_all(&frame(cmd, data)).is_ok()
    }

    fn at(&self, line: &str) -> bool {
        self.send(LINE, format!("AT#{line}\r\n").as_bytes())
    }
}

struct Session {
    chip: Chip,
    rt: Handle,
    profiles: Profiles,
    peer: String,
    local: [u8; 6],
    channel: Option<UnixStream>,
    hex: Arc<std::sync::atomic::AtomicBool>,
    unanswered: Option<Instant>,
}

impl Session {
    fn line(&mut self, line: &str) {
        println!("[ab5301] {line}");
        let (code, rest) = line.split_at(line.len().min(2));
        match code {
            "AD" => {
                if let Some(addr) = address(rest) {
                    self.peer = text(&addr);
                    *HELD.lock().unwrap() = Some((self.peer.clone(), false));
                    self.profiles.hfp.chip_link(&self.peer, true);
                    self.open(self.profiles.carplay.clone());
                    self.unanswered = Some(Instant::now() + IAP2_ANSWER);
                }
            }
            "IA" => {
                self.unanswered = None;
                if let Some((peer, _)) = HELD.lock().unwrap().take() {
                    self.profiles.hfp.chip_link(&peer, false);
                }
            }
            "UU" if rest.eq_ignore_ascii_case(IAP2) && !self.peer.is_empty() => {
                self.chip
                    .at(&format!("SP{}{IAP2}", self.peer.replace(':', "")));
            }
            "SI" => {
                self.hex.store(true, std::sync::atomic::Ordering::Relaxed);
                if let Some(bytes) = unhex(rest) {
                    self.data(&bytes);
                }
            }
            // The chip writes its own address lowest byte first.
            "MZ" => {
                if let Some(mut addr) = address(rest) {
                    addr.reverse();
                    self.local = addr;
                    if let Err(e) = std::fs::write(MODULE_BT_MAC, format!("{}\n", text(&addr))) {
                        eprintln!("[ab5301] {MODULE_BT_MAC}: {e}");
                    }
                }
            }
            "SV" => self.open(self.profiles.carplay.clone()),
            "SW" => self.open(self.profiles.android_auto.clone()),
            _ => {}
        }
    }

    fn tick(&mut self) {
        if self.unanswered.is_some_and(|until| Instant::now() >= until) {
            println!(
                "[ab5301] {} left iAP2 unanswered, its channel goes to android auto",
                self.peer
            );
            self.open(self.profiles.android_auto.clone());
        }
    }

    fn open(&mut self, to: Option<UnboundedSender<Arrival>>) {
        self.close();
        self.unanswered = None;
        let Some(to) = to else { return };
        let Ok((ours, theirs)) = UnixStream::pair() else {
            return;
        };
        let (Ok(reader), Ok(())) = (ours.try_clone(), theirs.set_nonblocking(true)) else {
            return;
        };
        let stream = {
            let _inside = self.rt.enter();
            match tokio::net::UnixStream::from_std(theirs) {
                Ok(s) => s,
                Err(_) => return,
            }
        };
        let (chip, hex) = (self.chip.clone(), self.hex.clone());
        std::thread::spawn(move || to_phone(reader, &chip, &hex));
        self.channel = Some(ours);
        println!(
            "[ab5301] {} opened a channel to {}",
            self.peer,
            text(&self.local)
        );
        let _ = to.send(Arrival {
            peer: self.peer.clone(),
            local: self.local,
            stream,
        });
    }

    fn close(&mut self) {
        if let Some(channel) = self.channel.take() {
            let _ = channel.shutdown(Shutdown::Both);
        }
    }

    /// Data without an announced channel are a phone starting iAP2.
    fn data(&mut self, bytes: &[u8]) {
        self.unanswered = None;
        if self.channel.is_none() {
            println!("[ab5301] data before a channel: {bytes:02x?}");
            self.open(self.profiles.carplay.clone());
        }
        if self
            .channel
            .as_mut()
            .is_some_and(|c| c.write_all(bytes).is_err())
        {
            self.close();
        }
    }
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

fn to_phone(mut from: UnixStream, chip: &Chip, hex: &std::sync::atomic::AtomicBool) {
    let mut buf = [0u8; CHUNK];
    while let Ok(n @ 1..) = from.read(&mut buf) {
        let sent = if hex.load(std::sync::atomic::Ordering::Relaxed) {
            buf[..n].chunks(HEX_CHUNK).all(|c| {
                let line: String = c.iter().map(|b| format!("{b:02X}")).collect();
                chip.at(&format!("SG{line}"))
            })
        } else {
            chip.send(DATA, &buf[..n])
        };
        if !sent {
            return;
        }
    }
}

fn serve(
    socket: &PathBuf,
    name: &str,
    wifi: &str,
    rt: &Handle,
    profiles: Profiles,
) -> Result<(), String> {
    let port = UnixStream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let mut from = port.try_clone().map_err(|e| e.to_string())?;
    from.set_read_timeout(Some(TICK))
        .map_err(|e| e.to_string())?;
    let chip = Chip(Arc::new(Mutex::new(port)));
    *PORT.lock().unwrap() = Some(chip.clone());
    if !crate::dongle::ap_on(wifi) {
        println!("[ab5301] access point not up, the chip's own reconnect stops");
        chip.at("CD");
    }
    chip.at(&format!("MM{name}"));
    for line in ["MY", "MZ", "CY", "CA"] {
        chip.at(line);
    }
    let mut session = Session {
        chip,
        rt: rt.clone(),
        profiles,
        peer: String::new(),
        local: [0; 6],
        channel: None,
        hex: Arc::default(),
        unanswered: None,
    };
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        match from.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
        while let Some(f) = next_frame(&mut buf) {
            let (dir, cmd, data) = (f[1], f[2], &f[4..f.len() - 1]);
            match (dir, cmd) {
                (CHIP, LINE) => String::from_utf8_lossy(data)
                    .split(['\r', '\n'])
                    .filter(|l| !l.is_empty())
                    .for_each(|l| session.line(l)),
                (CHIP, DATA) => session.data(data),
                _ => println!("[ab5301] {cmd:02x} {data:02x?}"),
            }
        }
        session.tick();
    }
    session.close();
    *PORT.lock().unwrap() = None;
    if let Some((peer, _)) = HELD.lock().unwrap().take() {
        session.profiles.hfp.chip_link(&peer, false);
    }
    Err("livi-core closed the chip's bluetooth".into())
}

pub fn up() -> bool {
    PORT.lock().unwrap().is_some()
}

/// The chip holds one phone, so whichever it is goes.
pub fn disconnect(mac: &str) -> Result<(), String> {
    let chip = PORT.lock().unwrap().clone();
    match chip {
        Some(chip) if chip.at("CD") => {
            println!("[ab5301] dropping {mac}");
            Ok(())
        }
        _ => Err("the chip's bluetooth is not up".into()),
    }
}

pub async fn run(socket: PathBuf, name: String, wifi: String, profiles: Profiles) {
    let rt = Handle::current();
    loop {
        let (socket, name, wifi, rt, profiles) = (
            socket.clone(),
            name.clone(),
            wifi.clone(),
            rt.clone(),
            profiles.clone(),
        );
        match tokio::task::spawn_blocking(move || serve(&socket, &name, &wifi, &rt, profiles)).await
        {
            Ok(Err(e)) => eprintln!("[ab5301] {e}"),
            Ok(Ok(())) => {}
            Err(_) => return,
        }
        tokio::time::sleep(RETRY).await;
    }
}

fn held(mac: &str) -> Option<bool> {
    HELD.lock()
        .unwrap()
        .as_ref()
        .map(|(peer, _)| peer.eq_ignore_ascii_case(mac))
}

fn load() -> f32 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}

pub struct Pager {
    wifi: String,
    ap_since: Mutex<Option<std::time::Instant>>,
}

impl Pager {
    pub fn new(wifi: String) -> Self {
        Self {
            wifi,
            ap_since: Mutex::default(),
        }
    }
}

impl Paging for Pager {
    fn ready(&self) -> bool {
        if PORT.lock().unwrap().is_none() {
            return false;
        }
        let mut since = self.ap_since.lock().unwrap();
        if !crate::dongle::ap_on(&self.wifi) {
            *since = None;
            return false;
        }
        let since = *since.get_or_insert_with(std::time::Instant::now);
        load() < CALM_LOAD || since.elapsed() >= CALM_WAIT
    }

    async fn link(&self, mac: &str) -> Link {
        match held(mac) {
            Some(true) => Link::Up,
            Some(false) => Link::Unknown,
            None => Link::Down,
        }
    }

    async fn poke(&self, mac: &str, _wake: Option<&str>) {
        let chip = PORT.lock().unwrap().clone();
        let mut held = HELD.lock().unwrap();
        if let (Some(chip), Some((peer, asked @ false))) = (chip, held.as_mut())
            && peer.eq_ignore_ascii_case(mac)
        {
            *asked = true;
            chip.at(&format!("SP{}{IAP2}", peer.replace(':', "")));
        }
    }

    async fn page(&self, mac: &str, _wake: Option<&str>) -> Option<bool> {
        let chip = PORT.lock().unwrap().clone()?;
        if !chip.at("CC") {
            return None;
        }
        println!("[ab5301] paging {mac}");
        let until = tokio::time::Instant::now() + PAGE_WAIT;
        while tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if held(mac).is_some() {
                break;
            }
        }
        Some(held(mac) == Some(true))
    }

    async fn disconnect(&self, mac: &str) {
        if let Err(e) = disconnect(mac) {
            eprintln!("[ab5301] {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_frame_splits_off_whole() {
        let mut buf = vec![0xff, 0x02, 0x81, 0xd4, 0x02, b'S', b'V'];
        buf.push(buf[2..].iter().fold(0, |x, b| x ^ b));
        assert_eq!(
            next_frame(&mut buf).map(|f| f[4..6].to_vec()),
            Some(b"SV".to_vec())
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn an_address_is_the_last_twelve_digits() {
        assert_eq!(
            address("4100000000B7AD"),
            Some([0x00, 0x00, 0x00, 0x00, 0xb7, 0xad])
        );
        assert_eq!(address("B7AD"), None);
    }
}
