// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The Bluetrum AB5301B as a head unit's controller: it owns the amplifier, its volume and the knob. It
//! talks frames of `02 dir cmd len data.. xor` (xor over dir to the last data byte) at 115200 8N1, dir
//! 0x80 from the host and 0x81 from the chip. Once initialised it sends `b2 00 ce` every 3 s and resets
//! the host after six without the `b0 ce 01` answer.
//!
//! Its classic bluetooth frames go to the helper and back over a socket.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::audio::system_volume::{Fault, OutputEvent};
use crate::system::keys::{self, Key};

pub const MODEL: &str = "AB5301B";

const HOST: u8 = 0x80;
const CHIP: u8 = 0x81;
/// The vendor firmware's maxVolume.
const MAX_LEVEL: f64 = 33.0;
/// Bluetooth and active source volume, as the vendor Config.ini sets them (btVolume, carplayVolume).
const BT_VOLUME: u8 = 30;
const SOURCE_VOLUME: u8 = 25;
const CENTRE: u8 = 7;
const INIT: &[(u8, &[u8])] = &[
    (0xa4, &[]),
    (0xa0, &[]),
    (0xb7, &[0x02]),
    (0xc3, &[]),
    (0xb3, &[0x00]),
    (0xba, &[]),
    (0xa7, &[0x0f, 0x00]),
];
const INIT_GAP: Duration = Duration::from_millis(300);
const STATUS_EVERY: Duration = Duration::from_secs(1);
const STATUS: [u8; 4] = [0x05, 0x00, 0x02, 0x00];
const KEY: u8 = 0xb2;
const KEY_ACK: u8 = 0xb0;
const ASKS_SOURCE: u8 = 0xc0;
const ALIVE: u8 = 0xce;
const KNOB_DOWN: u8 = 0x05;
const KNOB_UP: u8 = 0x04;
const PRESS: u8 = 0x02;
const LONG_PRESS: u8 = 0x01;
/// Each press arrives about three times.
const PRESS_REPEATS: Duration = Duration::from_millis(500);
/// Text lines, the pairing state and the RFCOMM data of the classic bluetooth.
const CLASSIC_BT: [u8; 4] = [0xd4, 0xd5, 0xd6, 0xdb];

struct Link {
    port: Mutex<File>,
    level: Mutex<Option<f64>>,
    watcher: Mutex<Option<mpsc::UnboundedSender<OutputEvent>>>,
    helper: Mutex<Option<UnixStream>>,
    pressed: Mutex<Option<(u8, Instant)>>,
}

fn frame(dir: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x02, dir, cmd, data.len() as u8];
    f.extend_from_slice(data);
    let xor = f[1..].iter().fold(0, |x, b| x ^ b);
    f.push(xor);
    f
}

fn raw_115200(port: &File) -> std::io::Result<()> {
    let fd = port.as_raw_fd();
    // SAFETY: termios is plain data, filled by tcgetattr before use.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: fd is an open tty and t a valid termios.
    if unsafe { libc::tcgetattr(fd, &mut t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: t is a valid termios.
    unsafe {
        libc::cfmakeraw(&mut t);
        libc::cfsetspeed(&mut t, libc::B115200);
    }
    t.c_cflag |= libc::CLOCAL | libc::CREAD;
    t.c_cc[libc::VMIN] = 1;
    t.c_cc[libc::VTIME] = 0;
    // SAFETY: fd is an open tty and t a valid termios.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

impl Link {
    fn send(&self, dir: u8, cmd: u8, data: &[u8]) {
        self.write(&frame(dir, cmd, data));
    }

    fn write(&self, f: &[u8]) {
        if let Err(e) = self.port.lock().unwrap().write_all(f) {
            eprintln!("[ab5301] {:02x} not sent: {e}", f[2]);
        }
    }

    fn apply(&self, level: f64) {
        let level = level.clamp(0.0, 1.0);
        *self.level.lock().unwrap() = Some(level);
        let master = (level * MAX_LEVEL).round() as u8;
        self.send(
            HOST,
            0xaa,
            &[
                master,
                0,
                CENTRE,
                CENTRE,
                0,
                0,
                BT_VOLUME,
                0,
                SOURCE_VOLUME,
                0,
            ],
        );
    }

    fn knob(&self, steps: f64) {
        let Some(level) = *self.level.lock().unwrap() else {
            return;
        };
        self.apply(level + steps / MAX_LEVEL);
        if let Some(tx) = self.watcher.lock().unwrap().as_ref() {
            let _ = tx.send(OutputEvent::Changed);
        }
    }

    fn press(&self, code: u8, key: Key) {
        let now = Instant::now();
        let before = self.pressed.lock().unwrap().replace((code, now));
        if !before.is_some_and(|(c, at)| c == code && now - at < PRESS_REPEATS) {
            keys::pressed(key);
        }
    }

    fn received(&self, f: &[u8]) {
        let (dir, cmd, data) = (f[1], f[2], &f[4..f.len() - 1]);
        match (dir, cmd, data) {
            (CHIP, KEY, [_, ALIVE, ..]) => self.send(HOST, KEY_ACK, &[ALIVE, 0x01, 0, 0, 0]),
            (CHIP, KEY, [_, KNOB_UP, ..]) => self.knob(1.0),
            (CHIP, KEY, [_, KNOB_DOWN, ..]) => self.knob(-1.0),
            (CHIP, KEY, [_, PRESS, ..]) => self.press(PRESS, Key::Display),
            // The chip silences the amplifier itself for about 2 s on a long press.
            (CHIP, KEY, [_, LONG_PRESS, ..]) => self.press(LONG_PRESS, Key::Mute),
            (CHIP, KEY, [_, key, ..]) => println!("[ab5301] key {key:02x}"),
            (CHIP, ASKS_SOURCE, _) => self.send(CHIP, ASKS_SOURCE, &[0, 0]),
            (CHIP, cmd, _) if CLASSIC_BT.contains(&cmd) => {
                let mut helper = self.helper.lock().unwrap();
                if helper.as_mut().is_some_and(|h| h.write_all(f).is_err()) {
                    *helper = None;
                }
            }
            _ => {}
        }
    }
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

fn each_frame(mut from: impl Read, mut take: impl FnMut(&[u8])) {
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    while let Ok(n @ 1..) = from.read(&mut chunk) {
        buf.extend_from_slice(&chunk[..n]);
        while let Some(f) = next_frame(&mut buf) {
            take(&f);
        }
    }
}

fn read_frames(link: &'static Link, port: File) {
    each_frame(port, |f| link.received(f));
    eprintln!("[ab5301] port closed");
}

fn serve_helper(link: &'static Link) {
    let path = livi_core_proto::helper::HelperSockets::system().module;
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[ab5301] {}: {e}", path.display());
            return;
        }
    };
    for helper in listener.incoming().flatten() {
        let Ok(from) = helper.try_clone() else {
            continue;
        };
        *link.helper.lock().unwrap() = Some(helper);
        std::thread::spawn(move || {
            each_frame(from, |f| {
                if f[1] == HOST {
                    link.write(f);
                }
            });
        });
    }
}

/// The error is `None` on a unit without the port.
fn link() -> Result<&'static Link, Fault> {
    static LINK: OnceLock<Result<Link, Option<String>>> = OnceLock::new();
    let link = LINK.get_or_init(|| {
        let path = livi_core_proto::helper::uart_module().ok_or(None)?;
        let name = path.display();
        let port = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                (e.kind() != std::io::ErrorKind::NotFound).then(|| format!("{name}: {e}"))
            })?;
        raw_115200(&port).map_err(|e| Some(format!("{name}: {e}")))?;
        let reader = port.try_clone().map_err(|e| Some(e.to_string()))?;
        Ok(Link {
            port: Mutex::new(port),
            level: Mutex::new(None),
            watcher: Mutex::new(None),
            helper: Mutex::new(None),
            pressed: Mutex::new(None),
        })
        .inspect(|_| started(reader))
    });
    link.as_ref().map_err(|e| match e {
        Some(e) => Fault::Other(e.clone()),
        None => Fault::Missing,
    })
}

fn started(reader: File) {
    std::thread::spawn(move || {
        let Ok(link) = link() else { return };
        std::thread::spawn(move || read_frames(link, reader));
        std::thread::spawn(move || serve_helper(link));
        for (cmd, data) in INIT {
            link.send(HOST, *cmd, data);
            std::thread::sleep(INIT_GAP);
        }
        println!("[ab5301] initialised");
        loop {
            link.send(HOST, 0xa5, &STATUS);
            std::thread::sleep(STATUS_EVERY);
        }
    });
}

pub async fn get(_device: &str) -> Result<f64, Fault> {
    link()?
        .level
        .lock()
        .unwrap()
        .ok_or_else(|| Fault::Other("no level set yet".into()))
}

pub async fn set(_device: &str, level: f64) -> Result<(), Fault> {
    link()?.apply(level);
    Ok(())
}

pub struct Watch;

impl Drop for Watch {
    fn drop(&mut self) {
        if let Ok(link) = link() {
            *link.watcher.lock().unwrap() = None;
        }
    }
}

pub fn watch(tx: mpsc::UnboundedSender<OutputEvent>) -> Watch {
    if let Ok(link) = link() {
        *link.watcher.lock().unwrap() = Some(tx);
    }
    Watch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_ends_in_the_xor_from_dir_on() {
        assert_eq!(frame(HOST, 0xa4, &[]), [0x02, 0x80, 0xa4, 0x00, 0x24]);
        assert_eq!(
            frame(HOST, KEY_ACK, &[ALIVE, 0x01, 0, 0, 0]),
            [0x02, 0x80, 0xb0, 0x05, 0xce, 0x01, 0x00, 0x00, 0x00, 0xfa]
        );
    }
}
