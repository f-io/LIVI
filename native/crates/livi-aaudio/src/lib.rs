// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#![cfg(target_os = "android")]

mod devices;
mod endpoint;
mod sink;
mod src;
mod stream;

use gstreamer as gst;
use gstreamer::prelude::*;

pub use devices::{Device, devices, set_lister};

pub fn register() -> Result<(), String> {
    gst::init().map_err(|e| e.to_string())?;
    gst::Element::register(
        None,
        "aaudiosink",
        gst::Rank::NONE,
        sink::AAudioSink::static_type(),
    )
    .map_err(|e| e.to_string())?;
    gst::Element::register(
        None,
        "aaudiosrc",
        gst::Rank::NONE,
        src::AAudioSrc::static_type(),
    )
    .map_err(|e| e.to_string())
}
