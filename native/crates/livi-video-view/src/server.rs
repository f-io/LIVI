// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use livi_host_proto::{
    Framer, UrlStatus, encode_reply, parse_camera_body, parse_plane_body, parse_url_body,
    ui_planes, url_status_body,
};
use livi_video_player::{CameraPlayer, Player, Say, UrlPlayer};

use crate::{control, screens};

const CHUNK: usize = 65536;
const STATUS_EVERY: Duration = Duration::from_millis(500);

fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path)
}

fn accept(name: &str, listener: UnixListener, serve: fn(UnixStream)) -> std::io::Result<()> {
    let name = name.to_string();
    std::thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let _ = std::thread::Builder::new()
                    .name(name.clone())
                    .spawn(move || serve(conn));
            }
        })?;
    Ok(())
}

pub fn serve(control: &Path, planes: &Path) -> std::io::Result<()> {
    accept("video-control", bind(control)?, serve_control)?;
    accept("video-planes", bind(planes)?, serve_plane)
}

fn serve_control(conn: UnixStream) {
    let Ok(mut answers) = conn.try_clone() else {
        return;
    };
    for line in BufReader::new(conn).lines() {
        let Ok(line) = line else { return };
        let Some(answer) = control::parse(&line).and_then(screens::apply) else {
            continue;
        };
        if answers.write_all(answer.as_bytes()).is_err() {
            return;
        }
    }
}

fn serve_plane(mut conn: UnixStream) {
    let mut framer = Framer::new();
    let mut chunk = vec![0u8; CHUNK];
    let mut drawn: Option<(u32, Arc<Player>)> = None;
    let mut playing: Option<(u32, Arc<UrlPlayer>)> = None;
    let mut filming: Option<(u32, Arc<CameraPlayer>)> = None;
    loop {
        let read = match conn.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        framer.push(&chunk[..read]);
        while let Some(m) = framer.next_message() {
            match m.op {
                ui_planes::CREATE => {
                    if let Some((id, player)) = drawn.take() {
                        screens::remove(id, &player);
                    }
                    drawn = parse_plane_body(&m.rest)
                        .and_then(|(codec, data)| screens::create(m.id, &codec, data))
                        .map(|player| (m.id, player));
                }
                ui_planes::FRAME => {
                    if let Some((_, player)) = &drawn {
                        player.push(&m.rest);
                    }
                }
                ui_planes::FLUSH => {
                    if let Some((_, player)) = &drawn {
                        player.flush();
                    }
                }
                ui_planes::PLAY_URL => {
                    if let Some((id, player)) = playing.take() {
                        screens::remove(id, &player);
                    }
                    let back = conn.try_clone().ok().map(|c| Arc::new(Mutex::new(c)));
                    let say = to_host(back.clone(), m.id);
                    playing = parse_url_body(&m.rest)
                        .and_then(|(url, device)| screens::play(m.id, &url, &device, say))
                        .map(|player| (m.id, player));
                    if let Some(back) = back {
                        report_status(back, m.id, playing.as_ref().map(|(_, p)| Arc::downgrade(p)));
                    }
                }
                ui_planes::PLAY_RATE => {
                    if let (Some((_, player)), Some(rate)) = (&playing, f64_of(&m.rest)) {
                        player.set_rate(rate);
                    }
                }
                ui_planes::PLAY_SEEK => {
                    if let (Some((_, player)), Some(to)) = (&playing, f64_of(&m.rest)) {
                        player.seek(to);
                    }
                }
                ui_planes::PLAY_MUTE => {
                    if let Some((_, player)) = &playing {
                        player.set_muted(m.rest.first() == Some(&1));
                    }
                }
                ui_planes::CAMERA => {
                    if let Some((id, camera)) = filming.take() {
                        screens::remove(id, &camera);
                    }
                    let back = conn.try_clone().ok().map(|c| Arc::new(Mutex::new(c)));
                    filming = parse_camera_body(&m.rest)
                        .and_then(|order| screens::camera(m.id, order, to_host(back, m.id)))
                        .map(|camera| (m.id, camera));
                }
                _ => {}
            }
        }
    }
    if let Some((id, player)) = drawn {
        screens::remove(id, &player);
    }
    if let Some((id, camera)) = filming {
        screens::remove(id, &camera);
    }
    if let Some((id, player)) = playing {
        screens::remove(id, &player);
    }
}

fn f64_of(rest: &[u8]) -> Option<f64> {
    Some(f64::from_le_bytes(rest.get(..8)?.try_into().ok()?))
}

type Back = Arc<Mutex<UnixStream>>;

fn send(back: &Back, op: u8, id: u32, rest: &[u8]) -> bool {
    let mut sock = back.lock().unwrap_or_else(|e| e.into_inner());
    sock.write_all(&encode_reply(op, id, rest)).is_ok()
}

fn to_host(back: Option<Back>, id: u32) -> Say {
    Arc::new(move |line| match &back {
        Some(back) => {
            send(back, ui_planes::LOG, id, line.as_bytes());
        }
        None => eprintln!("{line}"),
    })
}

fn report_status(back: Back, id: u32, player: Option<Weak<UrlPlayer>>) {
    let Some(player) = player else {
        let failed = UrlStatus {
            failed: true,
            ..UrlStatus::default()
        };
        send(&back, ui_planes::STATUS, id, &url_status_body(&failed));
        return;
    };
    let _ = std::thread::Builder::new()
        .name("video-status".into())
        .spawn(move || {
            while let Some(status) = player.upgrade().map(|p| p.status()) {
                if !send(&back, ui_planes::STATUS, id, &url_status_body(&status)) {
                    return;
                }
                std::thread::sleep(STATUS_EVERY);
            }
        });
}
