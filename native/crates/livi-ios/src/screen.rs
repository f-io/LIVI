// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

// UIKit tells the panel's pixels but not its size, the size follows from the pixel density of
// the model: 326 ppi on the iPad mini, 264 ppi on every other iPad iOS 15 runs on.

use std::ffi::CStr;

use objc2::MainThreadMarker;
use objc2_ui_kit::UIScreen;

const MM_PER_INCH: f64 = 25.4;
const MINI_PPI: f64 = 326.0;
const IPAD_PPI: f64 = 264.0;
const MINIS: [&str; 8] = [
    "iPad5,1", "iPad5,2", "iPad11,1", "iPad11,2", "iPad14,1", "iPad14,2", "iPad16,1", "iPad16,2",
];

fn machine() -> Option<String> {
    let mut buf = [0u8; 64];
    let mut len = buf.len();
    // SAFETY: buf holds len bytes, sysctlbyname writes at most that many.
    let code = unsafe {
        libc::sysctlbyname(
            c"hw.machine".as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if code != 0 {
        return None;
    }
    CStr::from_bytes_until_nul(&buf)
        .ok()
        .map(|s| s.to_string_lossy().into_owned())
}

fn ppi(machine: Option<&str>) -> f64 {
    match machine {
        Some(m) if MINIS.contains(&m) => MINI_PPI,
        _ => IPAD_PPI,
    }
}

// The app runs in landscape only, nativeBounds stays portrait.
pub fn report(mtm: MainThreadMarker) {
    #[allow(deprecated)]
    let size = UIScreen::mainScreen(mtm).nativeBounds().size;
    let (long, short) = (size.width.max(size.height), size.width.min(size.height));
    let machine = machine();
    let mm = |px: f64| px / ppi(machine.as_deref()) * MM_PER_INCH;
    println!("[ios] {} panel", machine.as_deref().unwrap_or("unknown"));
    livi_core::set_screen(mm(long), mm(short), long as usize, short as usize);
}
