//! Inner-UI child management: spawn with WAYLAND_DISPLAY set, SIGTERM on
//! shutdown, and the "restart" flow that re-execs the whole compositor.

use std::os::unix::process::CommandExt;

use nix::sys::signal::{kill, signal, SigHandler, Signal};
use nix::unistd::Pid;

use crate::state::LiviState;

pub fn spawn_startup(state: &mut LiviState) {
    // Auto-reap children so no zombies accumulate.
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

/// The inner UI ignored SIGTERM past the deadline: SIGKILL and stop the loop.
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
            let err = std::process::Command::new("/proc/self/exe").args(&argv[1..]).exec();
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
