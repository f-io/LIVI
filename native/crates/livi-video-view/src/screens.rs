// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use livi_host_proto::CameraOrder;
use livi_host_proto::display::Correction;
use livi_host_proto::plane_place;
use livi_video_player::{CameraPlayer, Player, Say, UrlPlayer};

use crate::control::{Line, Region};
use crate::main_thread::on_main;

unsafe extern "C" {
    fn livi_set_backdrop(parent: usize, r: f64, g: f64, b: f64);
    fn livi_prepare_window(ui: usize);
    fn livi_set_window_curves(ui: usize, rgb: *const f32, count: i32);
}

const CURVE_POINTS: usize = 256;

fn correct(window: usize, v: [f64; 5]) {
    let correction = Correction::from_line(v);
    if correction.neutral() {
        unsafe { livi_set_window_curves(window, std::ptr::null(), 0) };
        return;
    }
    let [r, g, b] = correction.curves(CURVE_POINTS);
    let rgb: Vec<f32> = (0..CURVE_POINTS)
        .flat_map(|i| [r[i] as f32, g[i] as f32, b[i] as f32])
        .collect();
    unsafe { livi_set_window_curves(window, rgb.as_ptr(), CURVE_POINTS as i32) };
}

pub trait View: Send + Sync {
    fn start(&self);
    fn stop(&self);
    fn place(&self, r: Region);
    fn set_visible(&self, shown: bool);
}

impl View for Player {
    fn start(&self) {
        Player::start(self)
    }

    fn stop(&self) {
        Player::stop(self)
    }

    fn place(&self, r: Region) {
        self.set_content_region(r.crop_l, r.crop_t, r.vis_w, r.vis_h, r.tier_w, r.tier_h);
    }

    fn set_visible(&self, shown: bool) {
        Player::set_visible(self, shown)
    }
}

impl View for UrlPlayer {
    fn start(&self) {
        UrlPlayer::start(self)
    }

    fn stop(&self) {
        UrlPlayer::stop(self)
    }

    fn place(&self, r: Region) {
        self.set_content_region(r.crop_l, r.crop_t, r.vis_w, r.vis_h, r.tier_w, r.tier_h);
    }

    fn set_visible(&self, shown: bool) {
        UrlPlayer::set_visible(self, shown)
    }
}

impl View for CameraPlayer {
    fn start(&self) {
        CameraPlayer::start(self)
    }

    fn stop(&self) {
        CameraPlayer::stop(self)
    }

    fn place(&self, r: Region) {
        self.set_content_region(r.crop_l, r.crop_t, r.vis_w, r.vis_h, r.tier_w, r.tier_h);
    }

    fn set_visible(&self, shown: bool) {
        CameraPlayer::set_visible(self, shown)
    }
}

#[derive(Clone, Copy)]
struct Placement {
    region: Region,
    shown: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            region: Region::default(),
            shown: true,
        }
    }
}

struct Drawn {
    tag: String,
    screen: &'static str,
    view: Arc<dyn View>,
}

#[derive(Default)]
struct Screens {
    windows: HashMap<String, usize>,
    placements: HashMap<String, Placement>,
    drawn: HashMap<u32, Drawn>,
    gamma: Option<[f64; 5]>,
    backdrop: Option<[u8; 3]>,
}

static SCREENS: LazyLock<Mutex<Screens>> = LazyLock::new(Mutex::default);

fn screens() -> MutexGuard<'static, Screens> {
    SCREENS.lock().unwrap_or_else(|e| e.into_inner())
}

fn paint(window: usize, [r, g, b]: [u8; 3]) {
    let c = |v: u8| f64::from(v) / 255.0;
    unsafe { livi_set_backdrop(window, c(r), c(g), c(b)) };
}

pub fn apply(line: Line) -> Option<String> {
    match line {
        Line::Claim(tag) => return Some(format!("bound {tag}\n")),
        Line::Place { tag, region } => on_main(|| {
            let mut s = screens();
            s.placements.entry(tag.clone()).or_default().region = region;
            s.drawn
                .values()
                .filter(|d| d.tag == tag)
                .for_each(|d| d.view.place(region));
        }),
        Line::Show { tag, shown } => on_main(|| {
            let mut s = screens();
            s.placements.entry(tag.clone()).or_default().shown = shown;
            s.drawn
                .values()
                .filter(|d| d.tag == tag)
                .for_each(|d| d.view.set_visible(shown));
        }),
        Line::Backdrop(rgb) => on_main(|| {
            let mut s = screens();
            s.backdrop = Some(rgb);
            s.windows.values().for_each(|&w| paint(w, rgb));
        }),
        Line::Gamma(v) => on_main(|| {
            let mut s = screens();
            s.gamma = Some(v);
            s.windows.values().for_each(|&w| correct(w, v));
        }),
    }
    None
}

pub fn create(id: u32, codec: &str, codec_data: &[u8]) -> Option<Arc<Player>> {
    draw(id, |window| Player::new(codec, window, codec_data))
}

pub fn play(id: u32, url: &str, audio_device: &str, say: Say) -> Option<Arc<UrlPlayer>> {
    draw(id, |window| UrlPlayer::new(url, audio_device, window, say))
}

pub fn camera(id: u32, order: CameraOrder, say: Say) -> Option<Arc<CameraPlayer>> {
    draw(id, move |window| CameraPlayer::new(&order, window, say))
}

fn draw<V: View + 'static>(
    id: u32,
    make: impl FnOnce(usize) -> Option<V> + Send,
) -> Option<Arc<V>> {
    let Some((tag, screen)) = plane_place(id) else {
        eprintln!("[video] plane 0x{id:x} belongs to no screen");
        return None;
    };
    let (view, replaced) = on_main(|| {
        let mut s = screens();
        let Some(&window) = s.windows.get(screen) else {
            eprintln!("[video] no {screen} window to draw plane 0x{id:x} in");
            return (None, None);
        };
        let Some(view) = make(window).map(Arc::new) else {
            return (None, None);
        };
        let at = s.placements.get(&tag).copied().unwrap_or_default();
        view.place(at.region);
        view.set_visible(at.shown);
        view.start();
        let replaced = s.drawn.insert(
            id,
            Drawn {
                tag,
                screen,
                view: view.clone(),
            },
        );
        (Some(view), replaced)
    });
    if let Some(old) = replaced {
        on_main(|| old.view.stop());
    }
    view
}

pub fn remove<V: View + 'static>(id: u32, view: &Arc<V>) {
    on_main(|| {
        let mut s = screens();
        if s.drawn
            .get(&id)
            .is_some_and(|d| std::ptr::addr_eq(Arc::as_ptr(&d.view), Arc::as_ptr(view)))
        {
            s.drawn.remove(&id);
        }
        drop(s);
        view.stop();
    });
}

pub fn set_window(screen: &str, window: usize) {
    on_main(|| {
        let mut s = screens();
        let before = if window == 0 {
            s.windows.remove(screen)
        } else {
            s.windows.insert(screen.to_string(), window)
        };
        let gone: Vec<Arc<dyn View>> = if before.is_some_and(|b| b != window) {
            let ids: Vec<u32> = s
                .drawn
                .iter()
                .filter(|(_, d)| d.screen == screen)
                .map(|(id, _)| *id)
                .collect();
            ids.iter()
                .filter_map(|id| s.drawn.remove(id))
                .map(|d| d.view)
                .collect()
        } else {
            Vec::new()
        };
        let (backdrop, gamma) = (s.backdrop, s.gamma);
        drop(s);
        gone.iter().for_each(|v| v.stop());
        if window != 0 {
            unsafe { livi_prepare_window(window) };
        }
        if let (true, Some(rgb)) = (window != 0, backdrop) {
            paint(window, rgb);
        }
        if let (true, Some(v)) = (window != 0, gamma) {
            correct(window, v);
        }
    });
}
