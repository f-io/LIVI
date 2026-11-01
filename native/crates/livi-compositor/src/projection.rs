// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::GlesTexture;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::backend::renderer::{ExportMem, Renderer};
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Point, Rectangle};
use smithay::wayland::compositor::{get_children, get_parent};
use smithay::wayland::shm::{BufferData, with_buffer_contents};

use crate::state::{Kind, LiviState};

const STREAM_TAG: &str = "main";
const MOVE_EVERY: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn normalize(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.x) / self.w, (y - self.y) / self.h)
    }
}

#[derive(Default)]
pub struct Fingers {
    held: BTreeMap<u32, (f64, f64)>,
    rect: Option<Rect>,
    moved: bool,
    sent: Option<Instant>,
}

fn visible_box(screen: (f64, f64, f64, f64), crop: Option<(f64, f64)>) -> Rect {
    let (sx, sy, ow, oh) = screen;
    let Some((vis_w, vis_h)) = crop else {
        return Rect {
            x: sx,
            y: sy,
            w: ow,
            h: oh,
        };
    };
    let scale = (ow / vis_w).min(oh / vis_h);
    let (w, h) = (vis_w * scale, vis_h * scale);
    Rect {
        x: sx + (ow - w) / 2.0,
        y: sy + (oh - h) / 2.0,
        w,
        h,
    }
}

fn stream_rect(state: &LiviState) -> Option<Rect> {
    let t = state
        .toplevels
        .iter()
        .find(|t| t.kind == Kind::Video && t.tag == STREAM_TAG && t.visible)?;
    let s = state.screens.get(t.screen_idx)?;
    let top = s.top_inset();
    let (ow, oh) = (f64::from(s.width), f64::from(s.height - top));
    if ow <= 0.0 || oh <= 0.0 {
        return None;
    }
    let cropped = t.has_crop && t.vis_w > 0.0 && t.vis_h > 0.0;
    Some(visible_box(
        (f64::from(s.x), f64::from(top), ow, oh),
        cropped.then_some((t.vis_w, t.vis_h)),
    ))
}

fn root_of(surface: &WlSurface) -> WlSurface {
    let mut root = surface.clone();
    while let Some(parent) = get_parent(&root) {
        root = parent;
    }
    root
}

fn shm_alpha(ptr: *const u8, len: usize, data: BufferData, x: i32, y: i32) -> Option<u8> {
    if x < 0 || y < 0 || x >= data.width || y >= data.height {
        return None;
    }
    match data.format {
        wl_shm::Format::Argb8888 => {
            let at = data.offset as usize + y as usize * data.stride as usize + x as usize * 4 + 3;
            (at < len).then(|| unsafe { *ptr.add(at) })
        }
        wl_shm::Format::Xrgb8888 => Some(u8::MAX),
        _ => None,
    }
}

fn shm_all_transparent(ptr: *const u8, len: usize, data: BufferData) -> bool {
    if data.format != wl_shm::Format::Argb8888 || data.width <= 0 || data.height <= 0 {
        return false;
    }
    let row = data.width as usize * 4;
    let end = data.offset as usize + (data.height as usize - 1) * data.stride as usize + row;
    if end > len {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    (0..data.height as usize).all(|y| {
        let start = data.offset as usize + y * data.stride as usize;
        bytes[start..start + row]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 0)
    })
}

fn alpha_at(
    state: &mut LiviState,
    surface: &WlSurface,
    local: Point<f64, smithay::utils::Logical>,
) -> Option<u8> {
    let scale = with_renderer_surface_state(surface, |s| s.buffer_scale()).unwrap_or(1);
    let (bx, by) = (
        (local.x * f64::from(scale)) as i32,
        (local.y * f64::from(scale)) as i32,
    );
    let shm = with_renderer_surface_state(surface, |s| {
        let buffer = s.buffer()?;
        with_buffer_contents(buffer, |ptr, len, data| shm_alpha(ptr, len, data, bx, by)).ok()
    })
    .flatten();
    if let Some(alpha) = shm {
        return alpha;
    }
    let renderer = state.renderer.as_mut()?;
    let context = renderer.context_id();
    let texture =
        with_renderer_surface_state(surface, |s| s.texture::<GlesTexture>(context).cloned())
            .flatten()?;
    let mapping = renderer
        .copy_texture(
            &texture,
            Rectangle::new((bx, by).into(), (1, 1).into()),
            Fourcc::Abgr8888,
        )
        .ok()?;
    renderer.map_texture(&mapping).ok()?.get(3).copied()
}

fn ui_transparent_at(state: &mut LiviState, x: f64, y: f64) -> bool {
    let Some((surface, origin)) = crate::input::surface_at(state, x, y) else {
        return false;
    };
    let root = root_of(&surface);
    let Some(kind) = state
        .toplevels
        .iter()
        .find(|t| t.toplevel.wl_surface() == &root)
        .map(|t| t.kind.clone())
    else {
        return false;
    };
    match kind {
        Kind::Video => true,
        Kind::Ui => alpha_at(state, &surface, Point::from((x, y)) - origin) == Some(0),
        Kind::Dialog | Kind::Pending => false,
    }
}

pub fn ui_clear(surface: &WlSurface) -> bool {
    if !get_children(surface).is_empty() {
        return false;
    }
    with_renderer_surface_state(surface, |s| {
        let buffer = s.buffer()?;
        with_buffer_contents(buffer, shm_all_transparent).ok()
    })
    .flatten()
    .unwrap_or(false)
}

fn frame(held: &BTreeMap<u32, (f64, f64)>, changed: Option<(u32, &str)>) -> String {
    let mut line = String::from("touch main");
    for (id, (x, y)) in held {
        let phase = match changed {
            Some((c, phase)) if c == *id => phase,
            _ => "move",
        };
        line.push_str(&format!(" {id} {phase} {x:.5} {y:.5}"));
    }
    line.push('\n');
    line
}

fn send(state: &mut LiviState, changed: Option<(u32, &str)>) {
    let line = frame(&state.fingers.held, changed);
    state.fingers.moved = false;
    state.fingers.sent = Some(Instant::now());
    crate::ctrl::send(state, &line);
}

pub fn claim(state: &mut LiviState, id: u32, x: f64, y: f64) -> bool {
    let Some(rect) = stream_rect(state) else {
        return false;
    };
    if !rect.contains(x, y) || !ui_transparent_at(state, x, y) {
        return false;
    }
    state.fingers.rect = Some(rect);
    state.fingers.held.insert(id, rect.normalize(x, y));
    send(state, Some((id, "down")));
    true
}

pub fn moved(state: &mut LiviState, id: u32, x: f64, y: f64) -> bool {
    let Some(rect) = state.fingers.rect else {
        return false;
    };
    let Some(point) = state.fingers.held.get_mut(&id) else {
        return false;
    };
    *point = rect.normalize(x, y);
    state.fingers.moved = true;
    flush(state);
    true
}

pub fn lifted(state: &mut LiviState, id: u32) -> bool {
    if !state.fingers.held.contains_key(&id) {
        return false;
    }
    send(state, Some((id, "up")));
    state.fingers.held.remove(&id);
    if state.fingers.held.is_empty() {
        state.fingers.rect = None;
    }
    true
}

pub fn cancel(state: &mut LiviState) {
    if state.fingers.held.is_empty() {
        return;
    }
    let mut line = String::from("touch main");
    for (id, (x, y)) in &state.fingers.held {
        line.push_str(&format!(" {id} cancel {x:.5} {y:.5}"));
    }
    line.push('\n');
    state.fingers = Fingers::default();
    crate::ctrl::send(state, &line);
}

pub fn flush(state: &mut LiviState) {
    let due = state
        .fingers
        .sent
        .is_none_or(|sent| sent.elapsed() >= MOVE_EVERY);
    if state.fingers.moved && due {
        send(state, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_box_matches_the_picture_inside_its_bars() {
        let full = visible_box((0.0, 0.0, 1280.0, 720.0), None);
        assert_eq!(
            full,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1280.0,
                h: 720.0
            }
        );
        let boxed = visible_box((0.0, 0.0, 1024.0, 768.0), Some((1280.0, 720.0)));
        assert_eq!(
            boxed,
            Rect {
                x: 0.0,
                y: 96.0,
                w: 1024.0,
                h: 576.0
            }
        );
        assert_eq!(boxed.normalize(512.0, 384.0), (0.5, 0.5));
        assert!(!boxed.contains(512.0, 50.0), "a bar is not the picture");
    }

    #[test]
    fn a_frame_names_every_held_finger_and_the_one_that_changed() {
        let mut held = BTreeMap::new();
        held.insert(0, (0.25, 0.5));
        held.insert(1, (0.75, 0.5));
        assert_eq!(
            frame(&held, Some((1, "down"))),
            "touch main 0 move 0.25000 0.50000 1 down 0.75000 0.50000\n"
        );
        assert_eq!(
            frame(&held, None),
            "touch main 0 move 0.25000 0.50000 1 move 0.75000 0.50000\n"
        );
    }

    fn shm(format: wl_shm::Format, pixels: &[[u8; 4]], width: i32) -> (Vec<u8>, BufferData) {
        let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
        let data = BufferData {
            offset: 0,
            width,
            height: pixels.len() as i32 / width,
            stride: width * 4,
            format,
        };
        (bytes, data)
    }

    #[test]
    fn only_a_zero_alpha_pixel_lets_the_touch_through() {
        let (bytes, data) = shm(
            wl_shm::Format::Argb8888,
            &[[0, 0, 0, 0], [10, 20, 30, 255]],
            2,
        );
        assert_eq!(shm_alpha(bytes.as_ptr(), bytes.len(), data, 0, 0), Some(0));
        assert_eq!(
            shm_alpha(bytes.as_ptr(), bytes.len(), data, 1, 0),
            Some(255)
        );
        assert_eq!(shm_alpha(bytes.as_ptr(), bytes.len(), data, 2, 0), None);
        let (bytes, data) = shm(wl_shm::Format::Xrgb8888, &[[0, 0, 0, 0]], 1);
        assert_eq!(
            shm_alpha(bytes.as_ptr(), bytes.len(), data, 0, 0),
            Some(255)
        );
    }

    #[test]
    fn a_ui_is_clear_only_when_every_pixel_is_transparent() {
        let (bytes, data) = shm(wl_shm::Format::Argb8888, &[[0; 4]; 4], 2);
        assert!(shm_all_transparent(bytes.as_ptr(), bytes.len(), data));
        let (bytes, data) = shm(
            wl_shm::Format::Argb8888,
            &[[0; 4], [0; 4], [0; 4], [0, 0, 0, 1]],
            2,
        );
        assert!(!shm_all_transparent(bytes.as_ptr(), bytes.len(), data));
        let (bytes, data) = shm(wl_shm::Format::Xrgb8888, &[[0; 4]; 4], 2);
        assert!(!shm_all_transparent(bytes.as_ptr(), bytes.len(), data));
    }
}
