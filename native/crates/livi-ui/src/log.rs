// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::FromRawFd;
use std::process::ChildStdin;
use std::thread;

use crate::clock;

pub fn to_core(mut core: ChildStdin) -> std::io::Result<()> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let [read, write] = fds;
    let terminal = unsafe { libc::dup(1) };
    unsafe {
        libc::dup2(write, 1);
        libc::dup2(write, 2);
        libc::close(write);
    }
    let (from, mut screen) = unsafe { (File::from_raw_fd(read), File::from_raw_fd(terminal)) };
    thread::Builder::new()
        .name("log-to-core".into())
        .spawn(move || {
            for line in BufReader::new(from).lines().map_while(Result::ok) {
                let stamped = format!("[{}] {line}\n", clock::stamp());
                let _ = screen.write_all(stamped.as_bytes());
                let _ = core.write_all(stamped.as_bytes());
            }
        })?;
    Ok(())
}
