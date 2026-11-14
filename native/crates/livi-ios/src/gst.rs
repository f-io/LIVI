// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

unsafe extern "C" {
    fn gst_init_static_plugins();
}

// iOS links every plugin into GStreamer.framework, so none is found by scanning.
pub fn init() -> Result<(), gstreamer::glib::Error> {
    gstreamer::init()?;
    // SAFETY: GStreamer is initialised and the call registers each plugin once.
    unsafe { gst_init_static_plugins() };
    Ok(())
}
