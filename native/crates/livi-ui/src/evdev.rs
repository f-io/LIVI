// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Multi-touch protocol B: LIVI OS has neither libinput nor a compositor.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;

const EV_SYN: u16 = 0x00;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0x00;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
const SLOTS: usize = 10;
const EVENT_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

/// `x` and `y` run from 0 to 1 across the panel in its own orientation.
#[derive(Debug, Clone, Copy)]
pub struct Touch {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub phase: Phase,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    down: bool,
    was_down: bool,
    moved: bool,
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

fn abs_info(file: &File, code: u16) -> Option<AbsInfo> {
    // EVIOCGABS(code) = _IOR('E', 0x40 + code, struct input_absinfo)
    let request = (2 << 30)
        | ((size_of::<AbsInfo>() as u32) << 16)
        | (u32::from(b'E') << 8)
        | (0x40 + u32::from(code));
    let mut info = AbsInfo::default();
    // SAFETY: EVIOCGABS fills exactly one input_absinfo.
    let ok = unsafe { libc::ioctl(file.as_raw_fd(), request as _, &mut info) } == 0;
    (ok && info.maximum > info.minimum).then_some(info)
}

fn open_touchscreen() -> Option<(File, AbsInfo, AbsInfo)> {
    (0..32).find_map(|n| {
        let file = File::open(format!("/dev/input/event{n}")).ok()?;
        let x = abs_info(&file, ABS_MT_POSITION_X)?;
        let y = abs_info(&file, ABS_MT_POSITION_Y)?;
        Some((file, x, y))
    })
}

pub fn touches(on: impl Fn(Touch) + Send + 'static) {
    let Some((mut file, ax, ay)) = open_touchscreen() else {
        eprintln!("[ui] no touchscreen in /dev/input");
        return;
    };
    let span = |v: i32, a: &AbsInfo| f64::from(v - a.minimum) / f64::from(a.maximum - a.minimum);
    std::thread::spawn(move || {
        let mut slots = [Slot::default(); SLOTS];
        let mut slot = 0;
        let mut buf = [0u8; EVENT_SIZE * 64];
        loop {
            let n = match file.read(&mut buf) {
                Ok(n) if n > 0 => n,
                _ => {
                    eprintln!("[ui] the touchscreen is gone");
                    return;
                }
            };
            for ev in buf[..n].as_chunks::<EVENT_SIZE>().0 {
                let kind = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                match (kind, code) {
                    (EV_ABS, ABS_MT_SLOT) => {
                        slot = usize::try_from(value).unwrap_or(0).min(SLOTS - 1)
                    }
                    (EV_ABS, ABS_MT_TRACKING_ID) => slots[slot].down = value >= 0,
                    (EV_ABS, ABS_MT_POSITION_X) => {
                        (slots[slot].x, slots[slot].moved) = (value, true)
                    }
                    (EV_ABS, ABS_MT_POSITION_Y) => {
                        (slots[slot].y, slots[slot].moved) = (value, true)
                    }
                    (EV_SYN, SYN_REPORT) => {
                        for (id, s) in slots.iter_mut().enumerate() {
                            let phase = match (s.was_down, s.down) {
                                (false, true) => Phase::Down,
                                (true, false) => Phase::Up,
                                (true, true) if s.moved => Phase::Move,
                                _ => continue,
                            };
                            (s.was_down, s.moved) = (s.down, false);
                            on(Touch {
                                id: id as u64,
                                x: span(s.x, &ax),
                                y: span(s.y, &ay),
                                phase,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    });
}
