// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "macos")]
use core::ffi::c_void;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn livi_run_on_main(work: extern "C" fn(*mut c_void), context: *mut c_void);
}

#[cfg(target_os = "macos")]
struct Job<F, R> {
    f: Option<F>,
    out: Option<R>,
}

#[cfg(target_os = "macos")]
extern "C" fn run<F: FnOnce() -> R, R>(context: *mut c_void) {
    // SAFETY: context is the &mut Job on_main passes, alive and unaliased until the call returns.
    let job = unsafe { &mut *context.cast::<Job<F, R>>() };
    if let Some(f) = job.f.take() {
        job.out = Some(f());
    }
}

#[cfg(in_process)]
pub fn on_main<F: FnOnce() -> R + Send, R: Send>(f: F) -> R {
    f()
}

#[cfg(target_os = "macos")]
pub fn on_main<F: FnOnce() -> R + Send, R: Send>(f: F) -> R {
    let mut job = Job {
        f: Some(f),
        out: None,
    };
    // SAFETY: livi_run_on_main runs synchronously, so job outlives the call.
    unsafe { livi_run_on_main(run::<F, R>, (&raw mut job).cast()) };
    job.out.expect("the main thread ran the job")
}
