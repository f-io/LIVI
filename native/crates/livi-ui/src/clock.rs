// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn now_text() -> String {
    // SAFETY: time accepts a null output pointer.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: an all-zero tm is valid, localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    unsafe { libc::localtime_r(&now, &mut tm) };
    let sep = if tm.tm_sec % 2 == 0 { ':' } else { ' ' };
    format!("{:02}{sep}{:02}", tm.tm_hour, tm.tm_min)
}

pub fn stamp() -> String {
    let mut tv: libc::timeval = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    // SAFETY: tv is a valid timeval, the timezone argument may be null.
    unsafe { libc::gettimeofday(&mut tv, std::ptr::null_mut()) };
    let now = tv.tv_sec;
    // SAFETY: an all-zero tm is valid, localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tv.tv_usec / 1000
    )
}
