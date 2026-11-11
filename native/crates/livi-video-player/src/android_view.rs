// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video::prelude::{VideoOverlayExt, VideoOverlayExtManual};

const FIRST_SURFACE: Duration = Duration::from_secs(2);

pub trait Views: Send + Sync {
    fn open(&self, rank: i32) -> Option<u64>;
    fn show(&self, view: u64, shown: bool);
    fn place(&self, view: u64, region: [f64; 6]);
    fn close(&self, view: u64);
}

#[derive(Default)]
struct Plane {
    sink: Option<gst::glib::WeakRef<gst::Element>>,
    window: usize,
}

static VIEWS: OnceLock<Box<dyn Views>> = OnceLock::new();
static PLANES: LazyLock<Mutex<HashMap<u64, Plane>>> = LazyLock::new(Mutex::default);
static SURFACED: Condvar = Condvar::new();

fn planes() -> MutexGuard<'static, HashMap<u64, Plane>> {
    PLANES.lock().unwrap_or_else(|e| e.into_inner())
}

fn bind(sink: &gst::Element, window: usize) {
    let Some(overlay) = sink.dynamic_cast_ref::<gstreamer_video::VideoOverlay>() else {
        return;
    };
    // SAFETY: window is 0 or an ANativeWindow that livi-android releases only after rebinding the plane.
    unsafe { overlay.set_window_handle(window) };
    if window != 0 {
        overlay.expose();
    }
}

pub fn set_views(views: impl Views + 'static) {
    let _ = VIEWS.set(Box::new(views));
}

pub fn set_surface(view: u64, window: usize) {
    let mut planes = planes();
    let plane = match planes.get_mut(&view) {
        Some(plane) => plane,
        None if window == 0 => return,
        None => planes.entry(view).or_default(),
    };
    plane.window = window;
    if let Some(sink) = plane.sink.as_ref().and_then(|s| s.upgrade()) {
        bind(&sink, window);
    }
    SURFACED.notify_all();
}

pub(crate) fn open(rank: i32, sink: &gst::Element) -> Option<u64> {
    let view = VIEWS.get()?.open(rank)?;
    let mut planes = planes();
    planes.entry(view).or_default().sink = Some(sink.downgrade());
    let (planes, _) = SURFACED
        .wait_timeout_while(planes, FIRST_SURFACE, |p| {
            p.get(&view).is_none_or(|p| p.window == 0)
        })
        .unwrap_or_else(|e| e.into_inner());
    match planes.get(&view).map(|p| p.window) {
        Some(window) if window != 0 => bind(sink, window),
        _ => eprintln!("[gst_video] view {view} got no surface"),
    }
    Some(view)
}

pub(crate) fn show(view: u64, shown: bool) {
    if let Some(views) = VIEWS.get() {
        views.show(view, shown);
    }
}

pub(crate) fn place(view: u64, region: [f64; 6]) {
    if let Some(views) = VIEWS.get() {
        views.place(view, region);
    }
}

pub(crate) fn close(view: u64) {
    planes().remove(&view);
    if let Some(views) = VIEWS.get() {
        views.close(view);
    }
}
