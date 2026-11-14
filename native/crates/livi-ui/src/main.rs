// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod bundle;
mod core_process;
mod log;

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use livi_core_client::{socket_path, wait_for_socket};
use livi_core_proto::LIFELINE_ENV;
use serde_json::Value;

use crate::core_process::{CoreProcess, RELAUNCH};

fn connect(
    started_by_core: bool,
    core: &mut Option<CoreProcess>,
    quit_code: &Arc<AtomicI32>,
    planes: &Path,
) -> std::io::Result<UnixStream> {
    let path = socket_path();
    if started_by_core || cfg!(not(target_os = "macos")) {
        return wait_for_socket(&path, Duration::from_secs(15));
    }
    if let Ok(s) = wait_for_socket(&path, Duration::from_secs(1)) {
        return Ok(s);
    }
    let code = quit_code.clone();
    *core = Some(CoreProcess::start(planes, move |c| {
        code.store(c.unwrap_or(0), Ordering::SeqCst);
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
    })?);
    wait_for_socket(&path, Duration::from_secs(15))
}

fn fixture() -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os("LIVI_UI_STATE") else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(b) = bundle::find() {
        b.use_bundled_gstreamer();
    }
    let started_by_core = std::env::var(LIFELINE_ENV).is_ok_and(|v| v == "1");
    let runtime = socket_path()
        .parent()
        .map(Path::to_path_buf)
        .ok_or("no runtime dir")?;
    std::fs::create_dir_all(&runtime)?;
    let planes = runtime.join("ui-planes.sock");
    let fixture = fixture()?;
    #[cfg(target_os = "macos")]
    if fixture.is_none() {
        livi_video_view::serve(&runtime.join("compositor.ctrl"), &planes)?;
    }

    let quit_code = Arc::new(AtomicI32::new(0));
    let mut core = None;
    livi_ui::run(
        || connect(started_by_core, &mut core, &quit_code, &planes),
        fixture,
        |_| {},
        None,
        false,
    )?;

    let code = match core {
        Some(core) => core
            .stop(Duration::from_secs(8))
            .unwrap_or(quit_code.load(Ordering::SeqCst)),
        None => quit_code.load(Ordering::SeqCst),
    };
    if code == RELAUNCH {
        if let Some(b) = bundle::find() {
            b.relaunch()?;
            return Ok(());
        }
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(std::env::current_exe()?).exec();
        return Err(err.into());
    }
    Ok(())
}
