// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::{CString, c_char, c_int};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;
use std::thread;

const INFO: c_int = 4;
const TAG: &std::ffi::CStr = c"livi";

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

pub fn write(line: &str) {
    let Ok(text) = CString::new(line.replace('\0', "")) else {
        return;
    };
    unsafe {
        __android_log_write(INFO, TAG.as_ptr(), text.as_ptr());
    }
}

pub fn capture() -> std::io::Result<()> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let [read, write_end] = fds;
    unsafe {
        libc::dup2(write_end, 1);
        libc::dup2(write_end, 2);
        libc::close(write_end);
    }
    let from = unsafe { File::from_raw_fd(read) };
    thread::Builder::new()
        .name("logcat".into())
        .spawn(move || {
            for line in BufReader::new(from).lines().map_while(Result::ok) {
                write(&line);
            }
        })?;
    Ok(())
}
