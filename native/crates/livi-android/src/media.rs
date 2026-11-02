// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::ffi::c_void;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use jni::sys::{JNIEnv, jobject};
use livi_core::EmbeddedHost;

pub struct InProcess;

impl EmbeddedHost for InProcess {
    fn serve(&self, sock: &Path) {
        livi_gst_host::process::serve(sock);
    }

    fn probe(&self) -> String {
        livi_gst_host::gst::probe_json()
    }
}

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut JNIEnv, surface: jobject) -> *mut c_void;
    fn ANativeWindow_release(window: *mut c_void);
}

static WINDOWS: LazyLock<Mutex<HashMap<u64, usize>>> = LazyLock::new(Mutex::default);

pub fn plane_surface(env: *mut JNIEnv, plane: u64, surface: jobject) {
    let window = if surface.is_null() {
        0
    } else {
        unsafe { ANativeWindow_fromSurface(env, surface) as usize }
    };
    let mut held = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    if window != 0 && !held.contains_key(&plane) {
        crate::black::paint(window);
    }
    livi_video_player::set_surface(plane, window);
    let before = if window == 0 {
        held.remove(&plane)
    } else {
        held.insert(plane, window)
    };
    if let Some(before) = before.filter(|&w| w != 0) {
        unsafe { ANativeWindow_release(before as *mut c_void) };
    }
}
