// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use livi_core::{Exit, Helper, Instance, OutputBackend, Paths, Resources};
use livi_helper::in_process::{Host, InProcessHelper};

const CORE_START: Duration = Duration::from_secs(15);

pub fn start() -> Result<(), Box<dyn std::error::Error>> {
    let mtm = objc2::MainThreadMarker::new().ok_or("not on the main thread")?;
    crate::scene::register(mtm);
    crate::screen::report(mtm);
    crate::gst::init()?;
    let socket = core()?;
    livi_ui::run(
        || livi_core_client::wait_for_socket(&socket, CORE_START),
        None,
        |_| {},
        Some(Box::new(crate::picker::pick)),
        true,
    )
}

fn core() -> Result<PathBuf, String> {
    let data = crate::paths::user_data()?;
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let paths = Paths::within(&data, crate::paths::runtime());
    let socket = paths.socket();
    livi_video_player::set_views(crate::planes::Planes);
    livi_video_view::set_window(livi_output_proto::SCREENS[0], crate::planes::STAGE);
    livi_video_view::serve(&paths.runtime.join("compositor.ctrl"))
        .map_err(|e| format!("no video control: {e}"))?;
    let host = Host {
        sockets: paths.helper.clone(),
        mfi: data.join("mfi"),
        lockdown: data.join("lockdown"),
        bt_keys: data.join("bluetooth-keys"),
    };
    let res = Resources {
        helper: Some(Helper::InProcess(Arc::new(move |start| {
            InProcessHelper::start(start, host.clone())
                .map(|helper| Box::new(helper) as Box<dyn Send>)
                .map_err(|e| format!("no helper thread: {e}"))
        }))),
        output: Some(OutputBackend::InProcess(Arc::new(crate::media::InProcess))),
        ..Resources::in_process()
    };
    let instance = Instance::claim(paths)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("no runtime: {e}"))?;
    std::thread::Builder::new()
        .name("core".into())
        .spawn(move || {
            match runtime.block_on(livi_core::run(instance, res, std::future::pending())) {
                Ok(Exit::Restart(_)) => {
                    println!("[core] asked for a restart, iOS relaunches by hand")
                }
                Ok(_) => {}
                Err(e) => eprintln!("[core] {e}"),
            }
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        })
        .map_err(|e| format!("no core thread: {e}"))?;
    Ok(socket)
}
