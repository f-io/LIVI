// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::unix::process::CommandExt;

use nix::sys::signal::{SigHandler, Signal, kill, signal};
use nix::unistd::Pid;

use crate::state::LiviState;

pub fn spawn_startup(state: &mut LiviState) {
    unsafe {
        let _ = signal(Signal::SIGCHLD, SigHandler::SigIgn);
    }
    let Some(cmd) = state.startup_cmd.clone() else {
        return;
    };
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd)
        .env("WAYLAND_DISPLAY", &state.ui_socket)
        .envs(
            state
                .render_node
                .as_ref()
                .map(|node| ("LIVI_RENDER_NODE", node)),
        )
        .spawn();
    match child {
        Ok(c) => state.startup_pid = Some(Pid::from_raw(c.id() as i32)),
        Err(e) => log::error!("failed to spawn startup command: {e}"),
    }
}

pub fn terminate_child(state: &mut LiviState) {
    if let Some(pid) = state.startup_pid {
        let _ = kill(pid, Signal::SIGTERM);
    }
}

pub fn force_restart(state: &mut LiviState) {
    log::info!("inner UI did not quit -> SIGKILL, then re-exec");
    state.restart_deadline = None;
    if let Some(pid) = state.startup_pid {
        let _ = kill(pid, Signal::SIGKILL);
    } else {
        state.running = false;
    }
}

pub fn shutdown(state: &mut LiviState) {
    if state.full_restart {
        terminate_child(state);
        log::info!("re-exec for full restart");
        let argv = state.saved_argv.clone();
        if !argv.is_empty() {
            let err = std::process::Command::new("/proc/self/exe")
                .args(&argv[1..])
                .exec();
            log::error!("re-exec via /proc/self/exe failed: {err}");
            let err = std::process::Command::new(&argv[0]).args(&argv[1..]).exec();
            log::error!("re-exec failed: {err}");
        }
    }
    terminate_child(state);
    if let Some(path) = &state.ctrl_path {
        let _ = std::fs::remove_file(path);
    }
}
