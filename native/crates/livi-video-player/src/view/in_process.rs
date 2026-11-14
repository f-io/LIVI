// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::in_process_view;

const FIT: &str = "force-aspect-ratio";

#[derive(Default)]
pub struct WindowView {
    view: AtomicU64,
    stretched: Mutex<Option<gst::Element>>,
}

impl WindowView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(&self, handle: usize, rank: i32, sink: Option<&gst::Element>) {
        if handle == 0 {
            return;
        }
        *self.stretched.lock().unwrap_or_else(|e| e.into_inner()) = sink
            .filter(|s| s.find_property(FIT).is_some() && !s.property::<bool>(FIT))
            .cloned();
        if let Some(view) = sink.and_then(|s| in_process_view::open(rank, s)) {
            self.view.store(view, Ordering::Release);
        }
    }

    pub fn set_visible(&self, visible: bool) {
        if let Some(view) = self.shown() {
            in_process_view::show(view, visible);
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
        // A stream without a region, like mirroring, keeps its aspect inside the plane.
        if let Some(sink) = &*self.stretched.lock().unwrap_or_else(|e| e.into_inner()) {
            let placed = vis_w > 0.0 && vis_h > 0.0 && tier_w > 0.0 && tier_h > 0.0;
            sink.set_property(FIT, !placed);
        }
        if let Some(view) = self.shown() {
            in_process_view::place(view, [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h]);
        }
    }

    pub fn remove(&self) {
        let view = self.view.swap(0, Ordering::AcqRel);
        if view != 0 {
            in_process_view::close(view);
        }
    }

    fn shown(&self) -> Option<u64> {
        Some(self.view.load(Ordering::Acquire)).filter(|&v| v != 0)
    }
}
