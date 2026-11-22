// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The Allwinner G2D, the 2D engine behind `/dev/g2d`.

use std::fs::File;
use std::os::fd::AsRawFd;

const CMD_BITBLT_H: libc::c_ulong = 0x55;
const FORMAT_NV12: u32 = 0x28;
pub const NONE: u32 = 0;
const ROT_90: u32 = 0x100;
const ROT_180: u32 = 0x200;
const ROT_270: u32 = 0x300;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Image {
    bbuff: i32,
    color: u32,
    format: u32,
    laddr: [u32; 3],
    haddr: [u32; 3],
    width: u32,
    height: u32,
    align: [u32; 3],
    clip_rect: Rect,
    resize: [u32; 2],
    coor: [i32; 2],
    gamut: u32,
    bpremul: i32,
    alpha: u8,
    pad: [u8; 3],
    mode: u32,
    fd: i32,
    use_phy_addr: i32,
    color_range: u32,
}

#[repr(C)]
struct Blt {
    flag: u32,
    src: Image,
    dst: Image,
}

pub fn nv12(fd: i32, width: u32, height: u32, clip: Rect) -> Image {
    Image {
        bbuff: 1,
        format: FORMAT_NV12,
        width,
        height,
        clip_rect: clip,
        alpha: 0xff,
        fd,
        ..Image::default()
    }
}

/// The blit flag that turns by `rotation` degrees.
pub fn turn(rotation: u32) -> u32 {
    match rotation {
        90 => ROT_90,
        180 => ROT_180,
        270 => ROT_270,
        _ => NONE,
    }
}

pub struct G2d(File);

impl G2d {
    pub fn open() -> Option<Self> {
        File::options()
            .read(true)
            .write(true)
            .open("/dev/g2d")
            .ok()
            .map(Self)
    }

    pub fn blit(&self, flag: u32, src: Image, dst: Image) -> bool {
        let mut blt = Blt { flag, src, dst };
        // SAFETY: blt is a g2d_blt_h the driver reads for the length of the call.
        unsafe { libc::ioctl(self.0.as_raw_fd(), CMD_BITBLT_H as _, &mut blt) >= 0 }
    }
}
