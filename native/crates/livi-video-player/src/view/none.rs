// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use gstreamer as gst;

#[derive(Default)]
pub struct WindowView;

impl WindowView {
    pub fn new() -> Self {
        Self
    }

    pub fn attach(&self, _handle: usize, _rank: i32, _sink: Option<&gst::Element>) {}

    pub fn set_visible(&self, _visible: bool) {}

    pub fn set_region(
        &self,
        _crop_l: f64,
        _crop_t: f64,
        _vis_w: f64,
        _vis_h: f64,
        _tier_w: f64,
        _tier_h: f64,
    ) {
    }

    pub fn remove(&self) {}
}
