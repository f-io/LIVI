// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! A 2D engine crops, scales and turns the NV12 frames into scanout buffers of the panel's size,
//! so a new format needs no new buffers.

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use drm::buffer::{Buffer, DrmFourcc, DrmModifier, Handle as BufferHandle, PlanarBuffer};
use drm::control::{
    self, AtomicCommitFlags, Device as _, atomic::AtomicModeReq, crtc, dumbbuffer::DumbBuffer,
    framebuffer, plane, property,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

const FRAMES: usize = 3;
const PLANE_OVERLAY: u64 = 0;
const PLANE_PRIMARY: u64 = 1;
const NV12: u32 = u32::from_le_bytes(*b"NV12");
const BUSY_TRIES: u32 = 5;
/// Each failed CMA try makes the kernel move pages out of CMA.
const ALLOC_RETRY: Duration = Duration::from_millis(500);
/// Two buffers already show the stream, the third is asked for this rarely.
const MIN_FRAMES: usize = 2;
const THIRD_RETRY: Duration = Duration::from_secs(5);
const RESERVE_TRIES: u32 = 50;
const RESERVE_WAIT: Duration = Duration::from_millis(200);

/// CMA again at a stream start would compete with the decoder for the same memory.
static SCANOUT: Mutex<Option<Vec<Frame>>> = Mutex::new(None);

const G2D_CMD_BITBLT_H: libc::c_ulong = 0x55;
const G2D_FORMAT_NV12: u32 = 0x28;
const G2D_BLT_NONE: u32 = 0;
const G2D_ROT_90: u32 = 0x100;
const G2D_ROT_180: u32 = 0x200;
const G2D_ROT_270: u32 = 0x300;

unsafe extern "C" {
    fn gst_is_dmabuf_memory(mem: *mut gst::ffi::GstMemory) -> i32;
    fn gst_dmabuf_memory_get_fd(mem: *mut gst::ffi::GstMemory) -> i32;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct G2dRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct G2dImage {
    bbuff: i32,
    color: u32,
    format: u32,
    laddr: [u32; 3],
    haddr: [u32; 3],
    width: u32,
    height: u32,
    align: [u32; 3],
    clip_rect: G2dRect,
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
struct G2dBlt {
    flag: u32,
    src: G2dImage,
    dst: G2dImage,
}

fn image(fd: i32, width: u32, height: u32, clip: G2dRect) -> G2dImage {
    G2dImage {
        bbuff: 1,
        format: G2D_FORMAT_NV12,
        width,
        height,
        clip_rect: clip,
        alpha: 0xff,
        fd,
        ..G2dImage::default()
    }
}

struct Card(OwnedFd);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl control::Device for Card {}

struct Nv12 {
    handle: BufferHandle,
    size: (u32, u32),
    pitch: u32,
}

impl PlanarBuffer for Nv12 {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn format(&self) -> DrmFourcc {
        DrmFourcc::Nv12
    }

    fn modifier(&self) -> Option<DrmModifier> {
        None
    }

    fn pitches(&self) -> [u32; 4] {
        [self.pitch, self.pitch, 0, 0]
    }

    fn handles(&self) -> [Option<BufferHandle>; 4] {
        [Some(self.handle), Some(self.handle), None, None]
    }

    fn offsets(&self) -> [u32; 4] {
        [0, self.pitch * self.size.1, 0, 0]
    }
}

struct Frame {
    dumb: DumbBuffer,
    prime: OwnedFd,
    fb: framebuffer::Handle,
}

#[derive(Clone, Copy, Default)]
struct Region {
    crop_l: f64,
    crop_t: f64,
    vis_w: f64,
    vis_h: f64,
    tier_w: f64,
    tier_h: f64,
}

struct Kms {
    card: Card,
    g2d: File,
    crtc: crtc::Handle,
    panel: (u32, u32),
    video: plane::Handle,
    atomic: Option<[property::Handle; 10]>,
    rotation: u32,
    frames: Vec<Frame>,
    retry_at: Option<Instant>,
    next: usize,
    on: bool,
}

fn prop(card: &Card, plane: plane::Handle, name: &str) -> Option<(property::Handle, u64)> {
    let set = card.get_properties(plane).ok()?;
    let (ids, values) = set.as_props_and_values();
    ids.iter().zip(values).find_map(|(&id, &v)| {
        let info = card.get_property(id).ok()?;
        (info.name().to_bytes() == name.as_bytes()).then_some((id, v))
    })
}

fn busy_retry(mut f: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match f() {
            Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
            r => return r,
        }
    }
}

fn card_of(handle: usize) -> Option<Card> {
    let fd = i32::try_from(handle).ok()?;
    // SAFETY: handle is the UI's open DRM device, it outlives this call and is duplicated here.
    Some(Card(
        unsafe { BorrowedFd::borrow_raw(fd) }
            .try_clone_to_owned()
            .ok()?,
    ))
}

fn lit_crtc(card: &Card, res: &control::ResourceHandles) -> Option<(crtc::Handle, control::Mode)> {
    res.crtcs()
        .iter()
        .find_map(|&c| Some((c, card.get_crtc(c).ok()?.mode()?)))
}

impl Kms {
    fn open(handle: usize) -> Option<Self> {
        let card = card_of(handle)?;
        let _ =
            drm::Device::set_client_capability(&card, drm::ClientCapability::UniversalPlanes, true);
        let res = card.resource_handles().ok()?;
        let (crtc, mode) = lit_crtc(&card, &res)?;
        let (pw, ph) = mode.size();
        let (mut video, mut primary) = (None, None);
        for p in card.plane_handles().ok()? {
            let Ok(info) = card.get_plane(p) else {
                continue;
            };
            if !res.filter_crtcs(info.possible_crtcs()).contains(&crtc) {
                continue;
            }
            match prop(&card, p, "type").map(|(_, v)| v) {
                Some(PLANE_OVERLAY) if video.is_none() && info.formats().contains(&NV12) => {
                    video = Some(p)
                }
                Some(PLANE_PRIMARY) => primary = Some(p),
                _ => {}
            }
        }
        let (video, primary) = (video?, primary?);
        if let Some(((v, _), (p, _))) = prop(&card, video, "zpos").zip(prop(&card, primary, "zpos"))
        {
            let set = (
                card.set_property(video, v, 0),
                card.set_property(primary, p, 1),
            );
            eprintln!(
                "[video] zpos set {set:?}, now video {:?} ui {:?}",
                prop(&card, video, "zpos").map(|(_, z)| z),
                prop(&card, primary, "zpos").map(|(_, z)| z)
            );
        }
        let atomic = drm::Device::set_client_capability(&card, drm::ClientCapability::Atomic, true)
            .ok()
            .and_then(|()| {
                [
                    "FB_ID", "CRTC_ID", "SRC_X", "SRC_Y", "SRC_W", "SRC_H", "CRTC_X", "CRTC_Y",
                    "CRTC_W", "CRTC_H",
                ]
                .iter()
                .map(|name| prop(&card, video, name).map(|(id, _)| id))
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()
            });
        let g2d = File::options()
            .read(true)
            .write(true)
            .open("/dev/g2d")
            .ok()?;
        let rotation = std::env::var("SLINT_KMS_ROTATION")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(0);
        Some(Self {
            card,
            g2d,
            crtc,
            panel: (u32::from(pw), u32::from(ph)),
            video,
            atomic,
            rotation,
            frames: Vec::new(),
            retry_at: None,
            next: 0,
            on: false,
        })
    }

    fn turned(&self) -> bool {
        self.rotation == 90 || self.rotation == 270
    }

    fn window(&self) -> (f64, f64) {
        let (w, h) = (f64::from(self.panel.0), f64::from(self.panel.1));
        if self.turned() { (h, w) } else { (w, h) }
    }

    /// The inverse of how linuxkms turns the window onto the panel.
    fn to_panel(&self, (x, y, w, h): (f64, f64, f64, f64)) -> (i32, i32, u32, u32) {
        let (pw, ph) = (f64::from(self.panel.0), f64::from(self.panel.1));
        let (px, py, rw, rh) = match self.rotation {
            90 => (pw - (y + h), x, h, w),
            180 => (pw - (x + w), ph - (y + h), w, h),
            270 => (y, ph - (x + w), h, w),
            _ => (x, y, w, h),
        };
        (
            px.round() as i32,
            py.round() as i32,
            rw.round().max(1.0) as u32,
            rh.round().max(1.0) as u32,
        )
    }

    fn release(&mut self) {
        for f in self.frames.drain(..) {
            let _ = self.card.destroy_framebuffer(f.fb);
            drop(f.prime);
            let _ = self.card.destroy_dumb_buffer(f.dumb);
        }
        self.retry_at = None;
    }

    fn allocate(&self, size: (u32, u32)) -> Option<Frame> {
        // The rotator writes rows of a multiple of 8 bytes.
        let width = size.0.next_multiple_of(8);
        let dumb = self
            .card
            .create_dumb_buffer((width, size.1 * 3 / 2), DrmFourcc::R8, 8)
            .ok()?;
        let nv12 = Nv12 {
            handle: dumb.handle(),
            size,
            pitch: dumb.pitch(),
        };
        let fb = self
            .card
            .add_planar_framebuffer(&nv12, control::FbCmd2Flags::empty());
        let prime = self
            .card
            .buffer_to_prime_fd(dumb.handle(), (libc::O_CLOEXEC | libc::O_RDWR) as u32);
        match (fb, prime) {
            (Ok(fb), Ok(prime)) => Some(Frame { dumb, prime, fb }),
            (fb, _) => {
                if let Ok(fb) = fb {
                    let _ = self.card.destroy_framebuffer(fb);
                }
                let _ = self.card.destroy_dumb_buffer(dumb);
                None
            }
        }
    }

    fn ensure(&mut self) -> bool {
        if self.frames.is_empty()
            && let Some(frames) = lock(&SCANOUT).take()
        {
            self.frames = frames;
        }
        if self.frames.len() == FRAMES {
            return true;
        }
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return self.frames.len() >= MIN_FRAMES;
        }
        while self.frames.len() < FRAMES {
            let Some(frame) = self.allocate(self.panel) else {
                break;
            };
            self.frames.push(frame);
        }
        if self.frames.len() < FRAMES {
            let shows = self.frames.len() >= MIN_FRAMES;
            let again = if shows { THIRD_RETRY } else { ALLOC_RETRY };
            eprintln!(
                "[video] {} of {FRAMES} scanout buffers for {}x{}, again in {} ms",
                self.frames.len(),
                self.panel.0,
                self.panel.1,
                again.as_millis()
            );
            self.retry_at = Some(Instant::now() + again);
            return shows;
        }
        self.retry_at = None;
        true
    }

    fn blit(&self, flag: u32, src: G2dImage, dst: G2dImage) -> bool {
        let mut blt = G2dBlt { flag, src, dst };
        // SAFETY: blt is a g2d_blt_h the driver reads for the length of the call.
        unsafe { libc::ioctl(self.g2d.as_raw_fd(), G2D_CMD_BITBLT_H as _, &mut blt) >= 0 }
    }

    fn hide(&mut self) {
        if !self.on {
            return;
        }
        let _ = busy_retry(|| {
            self.card
                .set_plane(self.video, self.crtc, None, 0, (0, 0, 0, 0), (0, 0, 0, 0))
        });
        self.on = false;
    }

    fn present(&mut self, sample: &gst::Sample, region: Region) {
        let Some(buffer) = sample.buffer() else {
            return;
        };
        let Some(info) = sample
            .caps()
            .and_then(|c| gst_video::VideoInfo::from_caps(c).ok())
        else {
            return;
        };
        let (stride, uv) = match buffer.meta::<gst_video::VideoMeta>() {
            Some(m) => (m.stride()[0], m.offset()[1]),
            None => (info.stride()[0], info.offset()[1]),
        };
        let Ok(stride) = u32::try_from(stride) else {
            return;
        };
        if buffer.n_memory() != 1 || stride == 0 {
            return;
        }
        let mem = buffer.peek_memory(0);
        // SAFETY: mem is a live memory of the buffer the sample holds.
        let fd = unsafe {
            let ptr = mem.as_mut_ptr();
            if gst_is_dmabuf_memory(ptr) == 0 {
                return;
            }
            gst_dmabuf_memory_get_fd(ptr)
        };

        let (sw, sh) = (f64::from(info.width()), f64::from(info.height()));
        let placed =
            region.vis_w > 0.0 && region.vis_h > 0.0 && region.tier_w > 0.0 && region.tier_h > 0.0;
        let (cx, cy, cw, ch) = if placed {
            let (kx, ky) = (sw / region.tier_w, sh / region.tier_h);
            (
                region.crop_l * kx,
                region.crop_t * ky,
                region.vis_w * kx,
                region.vis_h * ky,
            )
        } else {
            (0.0, 0.0, sw, sh)
        };
        // The rotator reads from 4 byte aligned addresses, chroma needs even sizes.
        let x = (cx.max(0.0) as u32) & !3;
        let y = (cy.max(0.0) as u32) & !1;
        let w = ((cw as u32).min(info.width().saturating_sub(x))) & !1;
        let h = ((ch as u32).min(info.height().saturating_sub(y))) & !1;
        if w == 0 || h == 0 {
            return;
        }
        if !self.ensure() {
            return;
        }

        let (ww, wh) = self.window();
        let scale = (ww / f64::from(w)).min(wh / f64::from(h));
        let dw = ((f64::from(w) * scale) as u32).max(2) & !1;
        let dh = ((f64::from(h) * scale) as u32).max(2) & !1;
        let rect = self.to_panel((
            (ww - f64::from(dw)) / 2.0,
            (wh - f64::from(dh)) / 2.0,
            f64::from(dw),
            f64::from(dh),
        ));
        let size = if self.turned() { (dh, dw) } else { (dw, dh) };

        self.next %= self.frames.len();
        let frame = &self.frames[self.next];
        let source = image(
            fd,
            stride,
            (uv / stride as usize) as u32,
            G2dRect {
                x: x as i32,
                y: y as i32,
                w,
                h,
            },
        );
        let target = image(
            frame.prime.as_raw_fd(),
            frame.dumb.pitch(),
            self.panel.1,
            G2dRect {
                x: 0,
                y: 0,
                w: size.0,
                h: size.1,
            },
        );
        let turn = match self.rotation {
            90 => G2D_ROT_90,
            180 => G2D_ROT_180,
            270 => G2D_ROT_270,
            _ => G2D_BLT_NONE,
        };
        let blitted = if turn == G2D_BLT_NONE {
            self.blit(G2D_BLT_NONE, source, target)
        } else if (dw, dh) == (w, h) {
            self.blit(turn, source, target)
        } else {
            // The rotator does not scale. The frame after the next one is off the screen, the
            // commit before waited for the flip, and holds the scaled frame unturned.
            if self.frames.len() < FRAMES {
                return;
            }
            let spare = &self.frames[(self.next + 1) % FRAMES];
            let pitch = (ww as u32).next_multiple_of(8);
            if u64::from(pitch) * wh as u64
                > u64::from(spare.dumb.pitch()) * u64::from(self.panel.1)
            {
                return;
            }
            let between = image(
                spare.prime.as_raw_fd(),
                pitch,
                wh as u32,
                G2dRect {
                    x: 0,
                    y: 0,
                    w: dw,
                    h: dh,
                },
            );
            self.blit(G2D_BLT_NONE, source, between) && self.blit(turn, between, target)
        };
        if !blitted {
            return;
        }

        let fb = frame.fb;
        let src = (0, 0, size.0 << 16, size.1 << 16);
        let shown = match self.atomic {
            Some(ids) => {
                let mut req = AtomicModeReq::new();
                let values = [
                    u64::from(u32::from(fb)),
                    u64::from(u32::from(self.crtc)),
                    u64::from(src.0),
                    u64::from(src.1),
                    u64::from(src.2),
                    u64::from(src.3),
                    rect.0 as i64 as u64,
                    rect.1 as i64 as u64,
                    u64::from(rect.2),
                    u64::from(rect.3),
                ];
                for (id, v) in ids.into_iter().zip(values) {
                    req.add_property(self.video, id, property::Value::UnsignedRange(v));
                }
                self.card.atomic_commit(AtomicCommitFlags::empty(), req)
            }
            None => self
                .card
                .set_plane(self.video, self.crtc, Some(fb), 0, rect, src),
        };
        match shown {
            Ok(()) => {
                if !self.on {
                    eprintln!("[video] first frame on the plane at {rect:?}");
                }
                self.on = true;
                self.next = (self.next + 1) % self.frames.len();
            }
            Err(e) => eprintln!("[video] plane: {e}"),
        }
    }
}

impl Drop for Kms {
    fn drop(&mut self) {
        self.hide();
        let mut kept = lock(&SCANOUT);
        if kept.is_none() && !self.frames.is_empty() {
            *kept = Some(std::mem::take(&mut self.frames));
        } else {
            drop(kept);
            self.release();
        }
    }
}

#[derive(Default)]
struct Shared {
    kms: Option<Kms>,
    region: Region,
    visible: bool,
}

#[derive(Default)]
pub struct WindowView {
    shared: Arc<Mutex<Shared>>,
}

fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

static CURVES: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// The CRTC's gamma table corrects UI and video alike at no cost per frame.
pub fn curves(handle: usize, rgb: &[f32]) {
    *lock(&CURVES) = rgb.to_vec();
    if set_curves(handle) {
        return;
    }
    // The UI sets the mode with its first frame, until then no CRTC is lit.
    std::thread::spawn(move || {
        for _ in 0..RESERVE_TRIES {
            std::thread::sleep(RESERVE_WAIT);
            if set_curves(handle) {
                return;
            }
        }
    });
}

fn set_curves(handle: usize) -> bool {
    let Some(card) = card_of(handle) else {
        return true;
    };
    let Some((crtc, _)) = card
        .resource_handles()
        .ok()
        .and_then(|res| lit_crtc(&card, &res))
    else {
        return false;
    };
    let size = card
        .get_crtc(crtc)
        .map(|info| info.gamma_length() as usize)
        .unwrap_or(0);
    if size < 2 {
        eprintln!("[video] the display has no gamma table, no colour correction");
        return true;
    }
    let rgb = lock(&CURVES).clone();
    let points = rgb.len() / 3;
    let channel = |c: usize| -> Vec<u16> {
        (0..size)
            .map(|i| {
                let v = if points < 2 {
                    i as f32 / (size - 1) as f32
                } else {
                    rgb[i * (points - 1) / (size - 1) * 3 + c]
                };
                (v.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
            })
            .collect()
    };
    let (r, g, b) = (channel(0), channel(1), channel(2));
    if let Err(e) = busy_retry(|| card.set_gamma(crtc, &r, &g, &b)) {
        eprintln!("[video] the display refused the gamma table: {e}");
    }
    true
}

/// During a stream start every CMA buffer costs seconds of page migration.
pub fn reserve(handle: usize) {
    if lock(&SCANOUT).is_some() {
        return;
    }
    // The UI sets the mode with its first frame, until then the CRTC has none.
    std::thread::spawn(move || {
        for _ in 0..RESERVE_TRIES {
            if let Some(mut kms) = Kms::open(handle) {
                if kms.ensure() {
                    eprintln!(
                        "[video] {} scanout buffers for {}x{} reserved",
                        kms.frames.len(),
                        kms.panel.0,
                        kms.panel.1
                    );
                }
                return;
            }
            std::thread::sleep(RESERVE_WAIT);
        }
        eprintln!("[video] no display mode to reserve scanout buffers for");
    });
}

impl WindowView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(&self, handle: usize, _rank: i32, sink: Option<&gst::Element>) {
        let Some(sink) = sink.and_then(|s| s.clone().downcast::<gst_app::AppSink>().ok()) else {
            return;
        };
        let Some(kms) = Kms::open(handle) else {
            eprintln!("[video] no NV12 plane or G2D on the window's display");
            return;
        };
        lock(&self.shared).kms = Some(kms);

        // Without video meta the decoder copies every frame out of its own buffers.
        if let Some(pad) = sink.static_pad("sink") {
            pad.add_probe(gst::PadProbeType::QUERY_DOWNSTREAM, |_, info| {
                if let Some(gst::PadProbeData::Query(q)) = &mut info.data
                    && let gst::QueryViewMut::Allocation(a) = q.view_mut()
                {
                    a.add_allocation_meta::<gst_video::VideoMeta>(None);
                }
                gst::PadProbeReturn::Ok
            });
        }
        let shared = self.shared.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let mut s = lock(&shared);
                    let (visible, region) = (s.visible, s.region);
                    if let Some(kms) = s.kms.as_mut()
                        && visible
                    {
                        kms.present(&sample, region);
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pub fn set_visible(&self, visible: bool) {
        let mut s = lock(&self.shared);
        s.visible = visible;
        if !visible && let Some(kms) = s.kms.as_mut() {
            kms.hide();
        }
    }

    pub fn set_region(
        &self,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    ) {
        lock(&self.shared).region = Region {
            crop_l,
            crop_t,
            vis_w,
            vis_h,
            tier_w,
            tier_h,
        };
    }

    pub fn remove(&self) {
        lock(&self.shared).kms.take();
    }
}
