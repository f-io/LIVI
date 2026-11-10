// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video::prelude::VideoOverlayExtManual;

unsafe extern "C" {
    fn livi_attach_view(parent: usize, rank: i32, out_view: *mut *mut c_void) -> usize;
    fn livi_remove_view(view: *mut c_void);
    fn livi_set_view_hidden(view: *mut c_void, hidden: bool);
    fn livi_set_content_region(
        view: *mut c_void,
        sink: *mut c_void,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    );
}

const FIT: &str = "force-aspect-ratio";

#[derive(Default)]
pub struct WindowView {
    view: AtomicPtr<c_void>,
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
        let mut view = null_mut();
        let overlay = unsafe { livi_attach_view(handle, rank, &mut view) };
        self.view.store(view, Ordering::Release);
        *self.stretched.lock().unwrap_or_else(|e| e.into_inner()) = sink
            .filter(|s| s.find_property(FIT).is_some() && !s.property::<bool>(FIT))
            .cloned();
        if overlay == 0 {
            return;
        }
        if let Some(o) = sink.and_then(|s| s.dynamic_cast_ref::<gstreamer_video::VideoOverlay>()) {
            unsafe { o.set_window_handle(overlay) };
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe { livi_set_view_hidden(self.view.load(Ordering::Acquire), !visible) };
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
        if let Some(sink) = &*self.stretched.lock().unwrap_or_else(|e| e.into_inner()) {
            let placed = vis_w > 0.0 && vis_h > 0.0 && tier_w > 0.0 && tier_h > 0.0;
            sink.set_property(FIT, !placed);
        }
        let view = self.view.load(Ordering::Acquire);
        if !view.is_null() {
            unsafe {
                livi_set_content_region(
                    view,
                    null_mut(),
                    crop_l,
                    crop_t,
                    vis_w,
                    vis_h,
                    tier_w,
                    tier_h,
                )
            }
        }
    }

    pub fn remove(&self) {
        self.stretched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let view = self.view.swap(null_mut(), Ordering::AcqRel);
        if !view.is_null() {
            unsafe { livi_remove_view(view) };
        }
    }
}
