// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Before an upload the UI goes, its memory (the decoders' CMA above all) is the upload's.

use std::fs::{self, File, OpenOptions};
use std::io::Cursor;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use drm::buffer::{Buffer, DrmFourcc};
use drm::control::atomic::AtomicModeReq;
use drm::control::{
    AtomicCommitFlags, Device as _, ResourceHandle, connector, framebuffer, plane, property,
};
use livi_base::lfwb::HOLD;

const UI: &str = "LIVI";
const STOP_WAIT: Duration = Duration::from_secs(8);
const KILL_WAIT: Duration = Duration::from_secs(2);
const SPLASH_MAX: Duration = Duration::from_secs(60);
const PLANE_PRIMARY: u64 = 1;
const LOGO: &[u8] = include_bytes!("../../livi-core/res/livi-logo.png");

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl drm::control::Device for Card {}

struct Shown {
    card: Card,
    plane: plane::Handle,
    fb: framebuffer::Handle,
}

pub fn splash() -> i32 {
    let Some(shown) = logo().and_then(show) else {
        return 1;
    };
    let _ = drm::Device::release_master_lock(&shown.card);
    // rcS waits for the parent: the UI opens the card after the master is free, a later opener never gets it.
    // SAFETY: livid has no other threads at this point.
    if unsafe { libc::fork() } != 0 {
        return 0;
    }
    let until = Instant::now() + SPLASH_MAX;
    while Instant::now() < until
        && shown
            .card
            .get_plane(shown.plane)
            .is_ok_and(|p| p.framebuffer() == Some(shown.fb))
    {
        thread::sleep(Duration::from_millis(200));
    }
    0
}

pub fn take() -> Result<(), String> {
    fs::write(HOLD, b"").map_err(|e| format!("{HOLD}: {e}"))?;
    signal(libc::SIGTERM);
    if !gone_within(STOP_WAIT) {
        signal(libc::SIGKILL);
        gone_within(KILL_WAIT);
    }
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    if shown.is_none() {
        *shown = logo().and_then(show);
    }
    Ok(())
}

pub fn give_back() {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).take();
    let _ = fs::remove_file(HOLD);
}

fn gone_within(wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while !running().is_empty() {
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
    true
}

fn running() -> Vec<i32> {
    let Ok(dir) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim_end() == UI)
        })
        .collect()
}

fn signal(sig: libc::c_int) {
    for pid in running() {
        // SAFETY: kill takes plain values.
        unsafe { libc::kill(pid, sig) };
    }
}

fn prop(card: &Card, of: impl ResourceHandle, name: &str) -> Option<(property::Handle, u64)> {
    let set = card.get_properties(of).ok()?;
    let (ids, values) = set.as_props_and_values();
    ids.iter().zip(values).find_map(|(&id, &v)| {
        let info = card.get_property(id).ok()?;
        (info.name().to_bytes() == name.as_bytes()).then_some((id, v))
    })
}

fn logo() -> Option<(Vec<[u8; 4]>, usize)> {
    let mut reader = png::Decoder::new(Cursor::new(LOGO)).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    if info.color_type != png::ColorType::Rgba || info.width != info.height {
        return None;
    }
    let px = rgba[..info.buffer_size()]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let on = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
            [on(b), on(g), on(r), 0]
        })
        .collect();
    Some((px, info.width as usize))
}

/// A plane of the logo's size in the middle, the mixer fills the rest black.
fn show((px, side): (Vec<[u8; 4]>, usize)) -> Option<Shown> {
    let card = Card(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/card0")
            .inspect_err(|e| eprintln!("[screen] /dev/dri/card0: {e}"))
            .ok()?,
    );
    drm::Device::set_client_capability(&card, drm::ClientCapability::UniversalPlanes, true).ok()?;
    drm::Device::set_client_capability(&card, drm::ClientCapability::Atomic, true).ok()?;
    let res = card.resource_handles().ok()?;
    let conn = res.connectors().iter().find_map(|&c| {
        card.get_connector(c, false)
            .ok()
            .filter(|i| i.state() == connector::State::Connected)
    })?;
    let mode = *conn.modes().first()?;
    let crtc = *res.crtcs().first()?;
    let plane = card.plane_handles().ok()?.into_iter().find(|&p| {
        card.get_plane(p)
            .is_ok_and(|i| res.filter_crtcs(i.possible_crtcs()).contains(&crtc))
            && prop(&card, p, "type").map(|(_, v)| v) == Some(PLANE_PRIMARY)
    })?;

    let mut buf = card
        .create_dumb_buffer((side as u32, side as u32), DrmFourcc::Xrgb8888, 32)
        .inspect_err(|e| eprintln!("[screen] buffer: {e}"))
        .ok()?;
    let pitch = buf.pitch() as usize;
    {
        let mut map = card.map_dumb_buffer(&mut buf).ok()?;
        draw(map.as_mut(), pitch, &px, side);
    }
    let fb = card.add_framebuffer(&buf, 24, 32).ok()?;

    let (pw, ph) = mode.size();
    let s = side as u64;
    let mut req = AtomicModeReq::new();
    req.add_property(
        conn.handle(),
        prop(&card, conn.handle(), "CRTC_ID")?.0,
        property::Value::CRTC(Some(crtc)),
    );
    req.add_property(
        crtc,
        prop(&card, crtc, "MODE_ID")?.0,
        card.create_property_blob(&mode).ok()?,
    );
    req.add_property(
        crtc,
        prop(&card, crtc, "ACTIVE")?.0,
        property::Value::Boolean(true),
    );
    for (name, value) in [
        ("FB_ID", property::Value::Framebuffer(Some(fb))),
        ("CRTC_ID", property::Value::CRTC(Some(crtc))),
        ("SRC_X", property::Value::UnsignedRange(0)),
        ("SRC_Y", property::Value::UnsignedRange(0)),
        ("SRC_W", property::Value::UnsignedRange(s << 16)),
        ("SRC_H", property::Value::UnsignedRange(s << 16)),
        (
            "CRTC_X",
            property::Value::SignedRange((i64::from(pw) - s as i64) / 2),
        ),
        (
            "CRTC_Y",
            property::Value::SignedRange((i64::from(ph) - s as i64) / 2),
        ),
        ("CRTC_W", property::Value::UnsignedRange(s)),
        ("CRTC_H", property::Value::UnsignedRange(s)),
    ] {
        req.add_property(plane, prop(&card, plane, name)?.0, value);
    }
    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
        .inspect_err(|e| eprintln!("[screen] commit: {e}"))
        .ok()?;
    Some(Shown { card, plane, fb })
}

/// Turns the logo on the panel the way linuxkms turns the UI.
fn draw(buf: &mut [u8], pitch: usize, px: &[[u8; 4]], side: usize) {
    let rotation: u32 = std::env::var("SLINT_KMS_ROTATION")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(0);
    let last = side - 1;
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i % side, i / side);
        let (x, y) = match rotation {
            90 => (last - y, x),
            180 => (last - x, last - y),
            270 => (y, last - x),
            _ => (x, y),
        };
        let o = y * pitch + x * 4;
        buf[o..o + 4].copy_from_slice(p);
    }
}
