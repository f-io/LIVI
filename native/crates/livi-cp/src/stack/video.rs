// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) async fn borrow_screen_for_video(&mut self) {
        if !self.video_playback || self.video_screen_borrowed {
            return;
        }
        println!("[cpStack] borrowing the screen for video");
        self.video_screen_borrowed = true;
        self.send_event_command(&video_playback::borrow_screen(true))
            .await;
    }

    pub(super) fn keep_side_message(&mut self, source: &str, message: &[u8]) {
        let Some(capture) = &mut self.video_capture else {
            return;
        };
        let kept = match capture.keep(source, message) {
            Ok(path) => format!("kept as {}", path.display()),
            Err(e) => format!("not kept: {e}"),
        };
        let what = match bplist::decode(message) {
            Ok(v) => video_playback::describe(&v),
            Err(_) => video_playback::describe_setting(message),
        };
        println!(
            "[cpStack] video {source} message: {what} ({} B, {kept})",
            message.len()
        );
    }

    pub(super) async fn on_stream_payload(&mut self, stream: &str, data: &[u8]) {
        let kept = match &mut self.video_capture {
            Some(capture) => match capture.keep(&format!("stream{stream}"), data) {
                Ok(path) => format!(", kept as {}", path.display()),
                Err(e) => format!(", not kept: {e}"),
            },
            None => String::new(),
        };
        let message = bplist::decode(data).ok();
        let polling = message
            .as_ref()
            .and_then(|m| m.get("type"))
            .and_then(Value::as_str)
            == Some("playbackInfo");
        if !polling || self.ctx.debug {
            let what = message.as_ref().map_or_else(
                || format!("{} B, not a plist", data.len()),
                video_playback::describe,
            );
            println!("[cpStack] data stream {stream} < {what}{kept}");
        }
        let Some(message) = message else { return };
        if !self.video_playback {
            return;
        }
        let status = self.ctx.media.video_status();
        let (answer, started) = self.video_player.handle(&message, status.as_ref());
        if let Some(answer) = answer {
            if !polling || self.ctx.debug {
                println!(
                    "[cpStack] data stream {stream} > {}",
                    video_playback::describe(&answer)
                );
            }
            let body = dict([(
                "params",
                dict([("data", Value::Data(bplist::encode(&answer)))]),
            )]);
            self.send_event_command_with(&body, &format!("X-Apple-StreamID: {stream}\r\n"))
                .await;
        }
        if started {
            println!("[cpStack] telling the phone that video plays");
            let status = self.video_status_word();
            self.send_event_command(&video_playback::allowed_command(self.video_allowed, status))
                .await;
            if self.video_allowed {
                self.borrow_screen_for_video().await;
            }
        }
        match message.get("type").and_then(Value::as_str) {
            Some("insertPlayQueueItem") => self.open_video_tunnel(&message).await,
            Some("stop") => self.video_tunnel = None,
            _ if self.video_tunnel.is_none() => {}
            Some("setRate") => self.ctx.media.set_video_rate(self.video_player.rate()),
            Some("seek") => self.ctx.media.seek_video(self.video_player.position()),
            Some("setProperty")
                if message.get("property").and_then(Value::as_str) == Some("muted") =>
            {
                if let Some(Value::Bool(muted)) = message.get("value") {
                    self.video_muted.store(*muted, Ordering::Relaxed);
                    self.ctx.media.set_video_muted(*muted);
                }
            }
            _ => {}
        }
        if message.get("type").and_then(Value::as_str) == Some("stop") && self.video_screen_borrowed
        {
            println!("[cpStack] video stopped, giving the screen back");
            self.video_screen_borrowed = false;
            self.send_event_command(&video_playback::borrow_screen(false))
                .await;
        }
    }

    pub(super) fn video_status_word(&self) -> &'static str {
        match (self.video_tunnel.is_some(), self.video_allowed) {
            (false, _) => "idle",
            (true, true) => "video",
            (true, false) => "audioOnly",
        }
    }

    pub(super) async fn set_video_allowed(&mut self, allowed: bool) {
        if self.video_allowed == allowed {
            return;
        }
        self.video_allowed = allowed;
        if !self.video_playback {
            return;
        }
        let status = self.video_status_word();
        if self.ctx.debug {
            let verb = if allowed { "allows" } else { "forbids" };
            println!("[cpStack] the vehicle {verb} video, telling the phone {status}");
        }
        self.ctx.media.set_video_shown(allowed);
        if self.live {
            self.send_event_command(&video_playback::allowed_command(allowed, status))
                .await;
        }
        if allowed && self.video_tunnel.is_some() {
            self.borrow_screen_for_video().await;
        }
    }

    pub(super) async fn open_video_tunnel(&mut self, message: &Value) {
        self.video_tunnel = None;
        let item = message.get("item");
        let Some(url) = item
            .and_then(|i| i.get("Content-Location"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let Some(offer) = self.video_proxy.clone() else {
            println!("[cpStack] no proxy from the phone, {url} is out of reach");
            return;
        };
        let (started_over, restarts) = mpsc::unbounded_channel();
        let gateway = match Gateway::open(Client::through(offer), url, started_over).await {
            Ok(gateway) => gateway,
            Err(e) => {
                println!("[cpStack] no gateway for video {url}: {e}");
                return;
            }
        };
        println!("[cpStack] video {url} reachable at {}", gateway.url());
        let device = self.ctx.config.borrow().audio_device.clone();
        let muted = item
            .and_then(|i| i.get("muted"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.video_muted.store(muted, Ordering::Relaxed);
        let media = self.ctx.media.clone();
        media.play_video(gateway.url().into(), device.clone(), muted);
        let start = self.video_player.position();
        if start > 0.0 {
            media.seek_video(start);
        }
        let restarts = tokio::spawn(reopen_video(
            media.clone(),
            gateway.url().into(),
            device,
            self.video_muted.clone(),
            restarts,
        ));
        let stop = Box::new(move || media.stop_video());
        self.video_tunnel = Some(VideoRun {
            _gateway: gateway,
            restarts,
            stop,
        });
    }

    pub(super) async fn relay_iap(&mut self, iap: &[u8]) {
        if let Some((_, wr)) = self.iap_relay.as_mut()
            && wr.write_all(iap).await.is_err()
        {
            self.iap_relay = None;
        }
    }
}
