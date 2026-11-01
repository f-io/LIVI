// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod cal;
mod ctrl;
mod deco;
mod drm;
mod host;
mod input;
mod layout;
mod projection;
mod render;
mod shell;
mod spawn;
mod state;

use std::time::Duration;

use smithay::reexports::calloop::EventLoop;

use crate::state::LiviState;

fn parse_args() -> Option<String> {
    let mut args = std::env::args().skip(1);
    let mut startup = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-s" => startup = args.next(),
            _ => {
                println!("Usage: livi-compositor [-s startup command]");
                std::process::exit(0);
            }
        }
    }
    startup
}

fn main() {
    env_logger::builder()
        .filter_level(if std::env::var("LIVI_WLR_DEBUG").is_ok() {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .filter_module("tracing::span", log::LevelFilter::Off)
        .init();

    let startup_cmd = parse_args();

    let mut event_loop: EventLoop<LiviState> =
        EventLoop::try_new().expect("failed to create event loop");

    let mut state = LiviState::new(&mut event_loop, startup_cmd);

    if drm::wanted() {
        drm::init(&mut state, &event_loop.handle());
    } else {
        host::init(&mut state, &event_loop.handle());
    }
    ctrl::init(&mut state, &event_loop.handle());
    ctrl::leave_with_core(&event_loop.handle());
    spawn::spawn_startup(&mut state);

    log::info!(
        "Running livi-compositor on WAYLAND_DISPLAY={}",
        state.ui_socket
    );

    while state.running {
        if event_loop
            .dispatch(Some(Duration::from_millis(16)), &mut state)
            .is_err()
        {
            log::error!("event loop error, shutting down");
            break;
        }
        state.after_dispatch();
    }

    spawn::shutdown(&mut state);
}
