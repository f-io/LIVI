// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::ffi::c_void;

unsafe extern "C" {
    fn livi_run_on_main(work: extern "C" fn(*mut c_void), context: *mut c_void);
}

struct Job<F, R> {
    f: Option<F>,
    out: Option<R>,
}

extern "C" fn run<F: FnOnce() -> R, R>(context: *mut c_void) {
    let job = unsafe { &mut *context.cast::<Job<F, R>>() };
    if let Some(f) = job.f.take() {
        job.out = Some(f());
    }
}

pub fn on_main<F: FnOnce() -> R + Send, R: Send>(f: F) -> R {
    let mut job = Job {
        f: Some(f),
        out: None,
    };
    unsafe { livi_run_on_main(run::<F, R>, (&raw mut job).cast()) };
    job.out.expect("the main thread ran the job")
}
