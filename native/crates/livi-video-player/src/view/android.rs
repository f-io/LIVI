// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::sync::atomic::{AtomicU64, Ordering};

use gstreamer as gst;

use crate::android_view;

#[derive(Default)]
pub struct WindowView {
    view: AtomicU64,
}

impl WindowView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(&self, handle: usize, rank: i32, sink: Option<&gst::Element>) {
        if handle == 0 {
            return;
        }
        if let Some(view) = sink.and_then(|s| android_view::open(rank, s)) {
            self.view.store(view, Ordering::Release);
        }
    }

    pub fn set_visible(&self, visible: bool) {
        if let Some(view) = self.shown() {
            android_view::show(view, visible);
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
        if let Some(view) = self.shown() {
            android_view::place(view, [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h]);
        }
    }

    pub fn remove(&self) {
        let view = self.view.swap(0, Ordering::AcqRel);
        if view != 0 {
            android_view::close(view);
        }
    }

    fn shown(&self) -> Option<u64> {
        Some(self.view.load(Ordering::Acquire)).filter(|&v| v != 0)
    }
}
