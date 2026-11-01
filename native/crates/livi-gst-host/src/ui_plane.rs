// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

use livi_host_proto::{
    CameraOrder, Framer, UrlStatus, camera_body, encode_reply, parse_url_status, plane_body,
    ui_planes, url_body,
};

use crate::{Plane, UrlPlay};

const QUEUE: usize = 1024;

fn connect(path: &str, id: u32, first: Vec<u8>) -> Option<(SyncSender<Vec<u8>>, UnixStream)> {
    let mut sock = UnixStream::connect(path)
        .inspect_err(|e| eprintln!("[gst-host] no UI for plane 0x{id:x} at {path}: {e}"))
        .ok()?;
    let back = sock.try_clone().ok()?;
    let (out, queued) = sync_channel::<Vec<u8>>(QUEUE);
    out.try_send(first).ok()?;
    std::thread::Builder::new()
        .name(format!("ui-plane-{id:x}"))
        .spawn(move || {
            for msg in queued {
                if sock.write_all(&msg).is_err() {
                    return;
                }
            }
        })
        .ok()?;
    Some((out, back))
}

pub struct UiPlane {
    id: u32,
    out: SyncSender<Vec<u8>>,
    lost: AtomicBool,
}

impl UiPlane {
    pub fn open(path: &str, id: u32, codec: &str, codec_data: &[u8]) -> Option<Self> {
        let create = encode_reply(ui_planes::CREATE, id, &plane_body(codec, codec_data));
        let (out, _) = connect(path, id, create)?;
        Some(Self {
            id,
            out,
            lost: AtomicBool::new(false),
        })
    }

    fn send(&self, op: u8, rest: &[u8]) {
        if let Err(TrySendError::Full(_)) = self.out.try_send(encode_reply(op, self.id, rest))
            && !self.lost.swap(true, Ordering::Relaxed)
        {
            eprintln!(
                "[gst-host] the UI stopped taking plane 0x{:x}, frames are lost",
                self.id
            );
        }
    }
}

impl Plane for UiPlane {
    fn start(&self) {}

    fn push(&self, nal: &[u8]) {
        self.send(ui_planes::FRAME, nal);
    }

    fn flush(&self) {
        self.send(ui_planes::FLUSH, &[]);
    }

    fn set_gamma(&self, _gamma: f64, _contrast: f64, _r: f64, _g: f64, _b: f64) {}
}

pub struct UiUrlPlayer {
    id: u32,
    out: SyncSender<Vec<u8>>,
    sock: UnixStream,
    status: Arc<Mutex<UrlStatus>>,
}

impl UiUrlPlayer {
    pub fn open(path: &str, id: u32, url: &str, audio_device: &str) -> Option<Self> {
        let play = encode_reply(ui_planes::PLAY_URL, id, &url_body(url, audio_device));
        let (out, sock) = connect(path, id, play)?;
        let status = Arc::new(Mutex::new(UrlStatus::default()));
        let (mut from_ui, heard) = (sock.try_clone().ok()?, status.clone());
        std::thread::Builder::new()
            .name(format!("ui-url-{id:x}"))
            .spawn(move || {
                let mut framer = Framer::new();
                let mut chunk = [0u8; 256];
                while let Ok(n @ 1..) = from_ui.read(&mut chunk) {
                    framer.push(&chunk[..n]);
                    while let Some(m) = framer.next_message() {
                        match m.op {
                            ui_planes::STATUS => {
                                if let Some(s) = parse_url_status(&m.rest) {
                                    *heard.lock().unwrap_or_else(|e| e.into_inner()) = s;
                                }
                            }
                            ui_planes::LOG => {
                                let line = String::from_utf8_lossy(&m.rest);
                                eprintln!("{}", livi_video_player::stamped(&line));
                            }
                            _ => {}
                        }
                    }
                }
                heard.lock().unwrap_or_else(|e| e.into_inner()).failed = true;
            })
            .ok()?;
        Some(Self {
            id,
            out,
            sock,
            status,
        })
    }

    fn send(&self, op: u8, rest: &[u8]) {
        let _ = self.out.try_send(encode_reply(op, self.id, rest));
    }
}

impl UrlPlay for UiUrlPlayer {
    fn start(&self) {}

    fn set_rate(&self, rate: f64) {
        self.send(ui_planes::PLAY_RATE, &rate.to_le_bytes());
    }

    fn seek(&self, seconds: f64) {
        self.send(ui_planes::PLAY_SEEK, &seconds.to_le_bytes());
    }

    fn set_muted(&self, muted: bool) {
        self.send(ui_planes::PLAY_MUTE, &[u8::from(muted)]);
    }

    fn status(&self) -> UrlStatus {
        *self.status.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for UiUrlPlayer {
    fn drop(&mut self) {
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

pub struct UiCamera {
    _out: SyncSender<Vec<u8>>,
    sock: UnixStream,
}

impl UiCamera {
    pub fn open(path: &str, id: u32, order: &CameraOrder) -> Option<Self> {
        let open = encode_reply(ui_planes::CAMERA, id, &camera_body(order));
        let (out, sock) = connect(path, id, open)?;
        let mut from_ui = sock.try_clone().ok()?;
        std::thread::Builder::new()
            .name(format!("ui-camera-{id:x}"))
            .spawn(move || {
                let mut framer = Framer::new();
                let mut chunk = [0u8; 256];
                while let Ok(n @ 1..) = from_ui.read(&mut chunk) {
                    framer.push(&chunk[..n]);
                    while let Some(m) = framer.next_message() {
                        if m.op == ui_planes::LOG {
                            let line = String::from_utf8_lossy(&m.rest);
                            eprintln!("{}", livi_video_player::stamped(&line));
                        }
                    }
                }
            })
            .ok()?;
        Some(Self { _out: out, sock })
    }
}

impl Drop for UiCamera {
    fn drop(&mut self) {
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    use livi_host_proto::{Framer, Message};

    use super::*;

    fn socket_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ui-plane-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn read_all(mut conn: UnixStream) -> Vec<Message> {
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut bytes = Vec::new();
        conn.read_to_end(&mut bytes).unwrap();
        let mut framer = Framer::new();
        framer.push(&bytes);
        std::iter::from_fn(|| framer.next_message()).collect()
    }

    #[test]
    fn the_ui_gets_the_create_then_the_frames_until_the_plane_goes() {
        let path = socket_path("frames");
        let listener = UnixListener::bind(&path).unwrap();
        let plane = UiPlane::open(path.to_str().unwrap(), 0x7a00_0001, "h264", &[1, 2]).unwrap();
        plane.push(&[5, 5]);
        plane.flush();
        drop(plane);

        let (conn, _) = listener.accept().unwrap();
        let got = read_all(conn);
        let _ = std::fs::remove_file(&path);
        let ops: Vec<(u8, u32)> = got.iter().map(|m| (m.op, m.id)).collect();
        assert_eq!(
            ops,
            [
                (ui_planes::CREATE, 0x7a00_0001),
                (ui_planes::FRAME, 0x7a00_0001),
                (ui_planes::FLUSH, 0x7a00_0001)
            ]
        );
        assert_eq!(got[0].rest, plane_body("h264", &[1, 2]));
        assert_eq!(got[1].rest, [5, 5]);
    }

    #[test]
    fn without_a_ui_there_is_no_plane() {
        let path = socket_path("nobody");
        assert!(UiPlane::open(path.to_str().unwrap(), 1, "h264", &[]).is_none());
        assert!(UiUrlPlayer::open(path.to_str().unwrap(), 1, "http://h/", "").is_none());
    }

    fn next(conn: &mut UnixStream, framer: &mut Framer) -> Message {
        let mut chunk = [0u8; 256];
        loop {
            if let Some(m) = framer.next_message() {
                return m;
            }
            let n = conn.read(&mut chunk).unwrap();
            assert!(n > 0, "the player hung up early");
            framer.push(&chunk[..n]);
        }
    }

    fn wait_for(player: &UiUrlPlayer, what: impl Fn(&UrlStatus) -> bool) {
        let start = std::time::Instant::now();
        while !what(&player.status()) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "status never came"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_ui_plays_the_url_takes_the_controls_and_reports_back() {
        let path = socket_path("url");
        let listener = UnixListener::bind(&path).unwrap();
        let player =
            UiUrlPlayer::open(path.to_str().unwrap(), 9, "http://h/a.m3u8", "car").unwrap();
        let (mut ui, _) = listener.accept().unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = ui.set_read_timeout(Some(Duration::from_secs(5)));
        let mut framer = Framer::new();

        let play = next(&mut ui, &mut framer);
        assert_eq!((play.op, play.id), (ui_planes::PLAY_URL, 9));
        assert_eq!(play.rest, url_body("http://h/a.m3u8", "car"));

        player.start();
        player.set_rate(0.0);
        player.seek(12.5);
        player.set_muted(true);
        let got: Vec<(u8, Vec<u8>)> = (0..3)
            .map(|_| next(&mut ui, &mut framer))
            .map(|m| (m.op, m.rest))
            .collect();
        assert_eq!(
            got,
            [
                (ui_planes::PLAY_RATE, 0.0f64.to_le_bytes().to_vec()),
                (ui_planes::PLAY_SEEK, 12.5f64.to_le_bytes().to_vec()),
                (ui_planes::PLAY_MUTE, vec![1]),
            ]
        );

        let playing = UrlStatus {
            position: Some(3.0),
            ready: true,
            playing: true,
            ..UrlStatus::default()
        };
        let status = encode_reply(
            ui_planes::STATUS,
            9,
            &livi_host_proto::url_status_body(&playing),
        );
        ui.write_all(&encode_reply(
            ui_planes::LOG,
            9,
            b"[gst_video] a line from the UI",
        ))
        .unwrap();
        ui.write_all(&status).unwrap();
        wait_for(&player, |s| *s == playing);

        drop(ui);
        wait_for(&player, |s| s.failed);
    }

    #[test]
    fn a_dropped_player_lets_the_ui_go() {
        let path = socket_path("drop");
        let listener = UnixListener::bind(&path).unwrap();
        let player = UiUrlPlayer::open(path.to_str().unwrap(), 9, "http://h/", "").unwrap();
        let (mut ui, _) = listener.accept().unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = ui.set_read_timeout(Some(Duration::from_secs(5)));
        assert_eq!(next(&mut ui, &mut Framer::new()).op, ui_planes::PLAY_URL);
        drop(player);
        assert_eq!(ui.read(&mut [0u8; 16]).unwrap(), 0);
    }
}
