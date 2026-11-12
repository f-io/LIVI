// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use livi_output_proto::ctrl::Command;

use crate::state::LiviState;

pub fn leave_with_core(handle: &LoopHandle<'static, LiviState>) {
    if !std::env::var("LIVI_LIFELINE").is_ok_and(|v| v == "1") {
        return;
    }
    handle
        .insert_source(
            Generic::new(std::io::stdin(), Interest::READ, Mode::Level),
            |_, _, state: &mut LiviState| {
                let mut buf = [0u8; 64];
                match std::io::stdin().read(&mut buf) {
                    Ok(n) if n > 0 => Ok(PostAction::Continue),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                        Ok(PostAction::Continue)
                    }
                    _ => {
                        log::info!("core is gone, the compositor stops");
                        state.running = false;
                        Ok(PostAction::Remove)
                    }
                }
            },
        )
        .expect("insert core lifeline");
}

pub fn init(state: &mut LiviState, handle: &LoopHandle<'static, LiviState>) {
    let Some(path) = state.ctrl_path.clone() else {
        return;
    };
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::error!("control socket bind failed: {e}");
            return;
        }
    };
    listener.set_nonblocking(true).ok();
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, state: &mut LiviState| {
                while let Ok((client, _)) = listener.accept() {
                    client.set_nonblocking(true).ok();
                    state.ctrl_client = Some(client);
                    state.ctrl_buf.clear();
                    state.ctrl_out.clear();
                    send_ui(state);
                    send_panels(state);
                    send_displays(state);
                }
                Ok(PostAction::Continue)
            },
        )
        .expect("insert ctrl listener");

    handle
        .insert_source(
            smithay::reexports::calloop::timer::Timer::from_duration(Duration::from_millis(20)),
            |_, _, state: &mut LiviState| {
                poll_client(state);
                flush_out(state);
                smithay::reexports::calloop::timer::TimeoutAction::ToDuration(
                    Duration::from_millis(20),
                )
            },
        )
        .expect("insert ctrl poll timer");
    log::info!("control socket at {path}");
}

fn poll_client(state: &mut LiviState) {
    let Some(client) = state.ctrl_client.as_mut() else {
        return;
    };
    let mut chunk = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match client.read(&mut buf) {
            Ok(0) => {
                state.ctrl_client = None;
                state.ctrl_buf.clear();
                return;
            }
            Ok(n) => chunk.extend_from_slice(&buf[..n]),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                state.ctrl_client = None;
                state.ctrl_buf.clear();
                return;
            }
        }
    }
    if chunk.is_empty() {
        return;
    }
    state.ctrl_buf.push_str(&String::from_utf8_lossy(&chunk));
    if state.ctrl_buf.len() > 64 * 1024 {
        log::warn!(
            "control buffer overflow, dropping {} bytes",
            state.ctrl_buf.len()
        );
        state.ctrl_buf.clear();
        return;
    }
    while let Some(pos) = state.ctrl_buf.find('\n') {
        let line: String = state.ctrl_buf.drain(..=pos).collect();
        let line = line.trim().to_string();
        if !line.is_empty() {
            log::debug!("ctrl < {line}");
            handle_line(state, &line);
        }
    }
}

pub fn send(state: &mut LiviState, line: &str) {
    state.ctrl_out.extend_from_slice(line.as_bytes());
    flush_out(state);
}

fn flush_out(state: &mut LiviState) {
    let LiviState {
        ctrl_client,
        ctrl_out,
        ..
    } = state;
    let Some(client) = ctrl_client.as_mut() else {
        ctrl_out.clear();
        return;
    };
    while !ctrl_out.is_empty() {
        match client.write(ctrl_out) {
            Ok(0) => break,
            Ok(n) => {
                ctrl_out.drain(..n);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                log::warn!("control write failed: {e}");
                *ctrl_client = None;
                ctrl_out.clear();
                return;
            }
        }
    }
}

fn send_ui(state: &mut LiviState) {
    let node = state
        .render_node
        .as_ref()
        .map_or_else(|| "-".to_string(), |p| p.display().to_string());
    let line = format!("ui {} {node}\n", state.ui_socket);
    send(state, &line);
}

pub fn send_displays(state: &mut LiviState) {
    if let Some(lines) = crate::drm::displays_lines(state) {
        send(state, &lines);
    }
}

pub fn send_panels(state: &mut LiviState) {
    let panels: Vec<String> = state
        .screens
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let (mm_w, mm_h) = crate::host::panel_mm(state, i)?;
            if s.width > 0 && s.height > 0 {
                Some(format!(
                    "panel {} {} {} {} {}\n",
                    s.role, mm_w, mm_h, s.width, s.height
                ))
            } else {
                None
            }
        })
        .collect();
    for p in panels {
        send(state, &p);
    }
}

fn handle_line(state: &mut LiviState, line: &str) {
    let Some(cmd) = Command::parse(line) else {
        if !line.is_empty() {
            log::info!("ctrl < {line} (not understood)");
        }
        return;
    };
    match cmd {
        Command::Restart => {
            log::info!("restart requested -> waiting for inner UI to quit, then re-exec");
            state.full_restart = true;
            if state.startup_pid.is_some() {
                state.restart_deadline = Some(Instant::now() + Duration::from_secs(8));
                crate::spawn::terminate_child(state);
            } else {
                state.running = false;
            }
        }
        Command::Screen { role, on, size } => {
            let Some(idx) = state.screen_idx_by_role(&role) else {
                return;
            };
            if let Some((w, h)) = size {
                state.screens[idx].req_width = w as i32;
                state.screens[idx].req_height = h as i32;
            }
            if on {
                crate::host::open_screen(state, idx);
            } else {
                crate::host::close_screen(state, idx);
            }
        }
        Command::Fullscreen { role, on } => {
            let Some(idx) = state.screen_idx_by_role(&role) else {
                return;
            };
            if state.drm.is_none() && state.screens[idx].fullscreen != on {
                log::info!("ctrl < {line}");
                crate::layout::set_fullscreen(state, idx, on);
            }
        }
        Command::Claim(tag) => {
            if crate::shell::bind_waiting_plane(state, &tag) {
                log::info!("ctrl < claim {tag} (took the plane already waiting)");
            } else if state.pending_video_tags.len() < 16 {
                state.pending_video_tags.push_back(tag.clone());
                log::info!(
                    "ctrl < claim {tag} (pending: {:?})",
                    state.pending_video_tags
                );
            }
        }
        Command::Unclaim(tag) => {
            state.pending_video_tags.retain(|t| *t != tag);
            log::info!(
                "ctrl < unclaim {tag} (pending: {:?})",
                state.pending_video_tags
            );
        }
        Command::Place {
            tag,
            screen,
            region: r,
        } => {
            log::info!("ctrl < {line}");
            {
                let cfg = state.cfg_for_tag(&tag);
                cfg.screen = screen;
                cfg.has_crop = r.vis_w > 0.0 && r.vis_h > 0.0;
                cfg.crop_l = r.crop_l;
                cfg.crop_t = r.crop_t;
                cfg.vis_w = r.vis_w;
                cfg.vis_h = r.vis_h;
                cfg.tier_w = r.tier_w;
                cfg.tier_h = r.tier_h;
            }
            if let Some(v) = state.find_video_by_tag(&tag) {
                crate::layout::apply_cfg_to_video(state, &tag, v);
            }
        }
        Command::Show { tag, shown } => {
            log::info!("ctrl < {line}");
            {
                let cfg = state.cfg_for_tag(&tag);
                cfg.has_visible = true;
                cfg.visible = shown;
            }
            if let Some(v) = state.find_video_by_tag(&tag) {
                state.toplevels[v].visible = shown;
                crate::host::damage_all(state);
            }
        }
        Command::Aspect { role, size } => {
            let Some(idx) = state.screen_idx_by_role(&role) else {
                return;
            };
            let aspect = size.map(|(w, h)| (w as i32, h as i32));
            if state.screens[idx].aspect != aspect {
                log::info!("ctrl < {line}");
                state.screens[idx].aspect = aspect;
                crate::host::refit(state, idx);
            }
        }
        Command::Mode { role, mode } => {
            let Some(idx) = state.screen_idx_by_role(&role) else {
                return;
            };
            log::info!("ctrl < {line}");
            crate::drm::set_mode(state, idx, mode);
        }
        Command::Backdrop([r, g, b]) => {
            let dbg = std::env::var("LIVI_DEBUG_BG").is_ok();
            for s in &mut state.screens {
                s.backdrop_color = [
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                ];
                s.has_backdrop_color = true;
            }
            if !dbg {
                crate::host::damage_all(state);
            }
        }
        Command::TouchCals(count) => crate::drm::touch_calibrations(state, count),
        Command::TouchCal { name, cal } => crate::drm::touch_calibration(state, name, cal),
        Command::Gamma(v) => {
            log::info!("ctrl < {line}");
            state.cal.gamma = v[0] as f32;
            state.cal.contrast = v[1] as f32;
            state.cal.gain = [v[2] as f32, v[3] as f32, v[4] as f32];
            state.cal.active = v.iter().any(|&v| v != 1.0);
            crate::drm::apply_calibration(state);
            crate::host::damage_all(state);
        }
    }
}
