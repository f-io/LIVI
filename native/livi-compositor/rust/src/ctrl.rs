//! Control socket (LIVI_COMPOSITOR_CTRL): line protocol from the host.
//! Commands: screen <role> <0|1> [w h] | claim <tag> | unclaim <tag> |
//! videocfg <tag> <screen> <crop...> | videoshow <tag> <0|1> |
//! backdrop <r> <g> <b> | gamma <g> <c> <r> <g> <b> | restart
//! Events out: "panel <role> <mm_w> <mm_h> <px_w> <px_h>" | "bound <tag>"

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use crate::state::LiviState;

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
                    send_panels(state);
                }
                Ok(PostAction::Continue)
            },
        )
        .expect("insert ctrl listener");

    // The client stream is polled from the loop turn: small and line-based.
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

/// A read can end mid-line, so the remainder is carried over to the next poll.
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
    // A peer that never sends a newline must not grow this without bound.
    if state.ctrl_buf.len() > 64 * 1024 {
        log::warn!("control buffer overflow, dropping {} bytes", state.ctrl_buf.len());
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

/// The socket is non-blocking, so writes are queued and retried on the next
/// poll. The host's claim handshake waits for the `bound` ack.
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
    if line == "restart" {
        log::info!("restart requested -> waiting for inner UI to quit, then re-exec");
        state.full_restart = true;
        if state.startup_pid.is_some() {
            state.restart_deadline = Some(Instant::now() + Duration::from_secs(8));
            crate::spawn::terminate_child(state);
        } else {
            state.running = false;
        }
        return;
    }

    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("screen") => {
            let Some(role) = parts.next() else { return };
            let Some(onoff) = parts.next().and_then(|v| v.parse::<i32>().ok()) else {
                return;
            };
            let w = parts.next().and_then(|v| v.parse::<i32>().ok());
            let h = parts.next().and_then(|v| v.parse::<i32>().ok());
            let Some(idx) = state.screen_idx_by_role(role) else {
                return;
            };
            if let (Some(w), Some(h)) = (w, h)
                && w > 0 && h > 0 {
                    state.screens[idx].req_width = w;
                    state.screens[idx].req_height = h;
                }
            if onoff != 0 {
                crate::host::open_screen(state, idx);
            } else {
                crate::host::close_screen(state, idx);
            }
        }
        Some("claim") => {
            if let Some(tag) = parts.next() {
                if crate::shell::bind_waiting_plane(state, tag) {
                    log::info!("ctrl < claim {tag} (took the plane already waiting)");
                } else if state.pending_video_tags.len() < 16 {
                    state.pending_video_tags.push_back(tag.to_string());
                    log::info!("ctrl < claim {tag} (pending: {:?})", state.pending_video_tags);
                }
            }
        }
        Some("unclaim") => {
            if let Some(tag) = parts.next() {
                state.pending_video_tags.retain(|t| t != tag);
                log::info!("ctrl < unclaim {tag} (pending: {:?})", state.pending_video_tags);
            }
        }
        Some("videocfg") => {
            log::info!("ctrl < {line}");
            let (Some(tag), Some(screen)) = (parts.next(), parts.next()) else {
                return;
            };
            let nums: Vec<f64> = parts.filter_map(|v| v.parse().ok()).collect();
            if nums.len() != 6 {
                return;
            }
            let tag = tag.to_string();
            {
                let cfg = state.cfg_for_tag(&tag);
                cfg.screen = screen.to_string();
                cfg.has_crop = nums[2] > 0.0 && nums[3] > 0.0;
                cfg.crop_l = nums[0];
                cfg.crop_t = nums[1];
                cfg.vis_w = nums[2];
                cfg.vis_h = nums[3];
                cfg.tier_w = nums[4];
                cfg.tier_h = nums[5];
            }
            if let Some(v) = state.find_video_by_tag(&tag) {
                crate::layout::apply_cfg_to_video(state, &tag, v);
            }
        }
        Some("videoshow") => {
            log::info!("ctrl < {line}");
            let (Some(tag), Some(onoff)) = (parts.next(), parts.next().and_then(|v| v.parse::<i32>().ok()))
            else {
                return;
            };
            let tag = tag.to_string();
            {
                let cfg = state.cfg_for_tag(&tag);
                cfg.has_visible = true;
                cfg.visible = onoff != 0;
            }
            if let Some(v) = state.find_video_by_tag(&tag) {
                state.toplevels[v].visible = onoff != 0;
                crate::host::damage_all(state);
            }
        }
        Some("backdrop") => {
            let nums: Vec<i32> = parts.filter_map(|v| v.parse().ok()).collect();
            if nums.len() != 3 {
                return;
            }
            let dbg = std::env::var("LIVI_DEBUG_BG").is_ok();
            for s in &mut state.screens {
                s.backdrop_color = [
                    nums[0] as f32 / 255.0,
                    nums[1] as f32 / 255.0,
                    nums[2] as f32 / 255.0,
                    1.0,
                ];
                s.has_backdrop_color = true;
            }
            if !dbg {
                crate::host::damage_all(state);
            }
        }
        Some("gamma") => {
            log::info!("ctrl < {line}");
            let nums: Vec<f64> = parts.filter_map(|v| v.parse().ok()).collect();
            if nums.len() != 5 {
                return;
            }
            state.cal.gamma = nums[0] as f32;
            state.cal.contrast = nums[1] as f32;
            state.cal.gain = [nums[2] as f32, nums[3] as f32, nums[4] as f32];
            state.cal.active = nums.iter().any(|&v| v != 1.0);
            crate::host::damage_all(state);
        }
        _ => {}
    }
}
