// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "macos")]
pub fn promote(label: &str) {
    use std::time::Duration;

    const PERIOD: Duration = Duration::from_millis(10);
    const COMPUTATION: Duration = Duration::from_millis(3);

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }

    let mut tb = Timebase { numer: 0, denom: 0 };
    // SAFETY: tb is a valid out pointer for the call.
    if unsafe { mach_timebase_info(&mut tb) } != 0 || tb.numer == 0 {
        return;
    }
    let ticks =
        |d: Duration| (d.as_nanos() as u64 * u64::from(tb.denom) / u64::from(tb.numer)) as u32;
    let mut policy = libc::thread_time_constraint_policy {
        period: ticks(PERIOD),
        computation: ticks(COMPUTATION),
        constraint: ticks(PERIOD),
        preemptible: 1,
    };
    // SAFETY: the policy is a time-constraint policy of the stated count for the calling thread.
    let rc = unsafe {
        libc::thread_policy_set(
            libc::pthread_mach_thread_np(libc::pthread_self()),
            libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
            (&raw mut policy).cast(),
            libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if rc != 0 {
        eprintln!("[audio] {label}: real-time scheduling denied (kern {rc}), normal priority");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn promote(label: &str) {
    // SAFETY: an all-zero sched_param is valid.
    let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
    param.sched_priority = 10;
    // SAFETY: param is a valid sched_param for the calling thread.
    let rc = unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) };
    if rc != 0 {
        eprintln!("[audio] {label}: real-time priority denied (rc={rc}), normal priority");
    }
}
