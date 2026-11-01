// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) async fn handle_command(&mut self, req: &Request) -> Response {
        let body = bplist::decode(&req.body).unwrap_or(Value::Dict(Vec::new()));
        let kind = body
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let params = body
            .get("params")
            .cloned()
            .unwrap_or(Value::Dict(Vec::new()));
        match kind.as_str() {
            "requestUI" => {
                let url = params.get("url").and_then(Value::as_str).unwrap_or("");
                if url == "videoplayback:" {
                    println!("[cpStack] requestUI {url}, the phone wants to play a video");
                    self.borrow_screen_for_video().await;
                } else {
                    println!("[cpStack] requestUI {url} → host UI requested");
                    self.emit(StackEvent::HostUiRequested);
                }
            }
            "disableBluetooth" => {
                let device = params.get("deviceID").and_then(Value::as_str).unwrap_or("");
                println!("[cpStack] disableBluetooth (deviceID={device}) — disconnecting BT");
                self.emit(StackEvent::DisableBluetooth(device.to_string()));
            }
            "modesChanged" => {
                self.modes_changed(&params);
                if self.ctx.debug {
                    println!("[cpStack] modesChanged {params:?}");
                }
            }
            "changeVideoPlaybackToAudioOnly" => {
                if self.ctx.debug {
                    println!("[cpStack] the phone moves the video to sound only");
                }
            }
            "suggestUI" => {
                let urls = params
                    .get("urls")
                    .and_then(Value::as_array)
                    .map_or(0, <[Value]>::len);
                println!("[cpStack] suggestUI ({urls} urls, not shown)");
            }
            "iAPSendMessage" => {
                if let Some(data) = params.get("data").and_then(Value::as_data) {
                    let data = data.to_vec();
                    self.relay_iap(&data).await;
                }
            }
            "duckAudio" => {
                let duration = number(params.get("durationMs"));
                let db = number(params.get("volume"));
                let level = 10f64.powf(db / 20.0).clamp(0.0, 1.0);
                println!(
                    "[cpStack] duckAudio volume={db}dB (level={level:.3}) durationMs={duration}"
                );
                self.emit(StackEvent::Duck {
                    level,
                    duration_ms: duration.max(0.0) as u32,
                });
            }
            "unduckAudio" => {
                let duration = number(params.get("durationMs"));
                println!("[cpStack] unduckAudio durationMs={duration}");
                self.emit(StackEvent::Duck {
                    level: 1.0,
                    duration_ms: duration.max(0.0) as u32,
                });
            }
            "setProxyParameters" => {
                let scope = match self.peer {
                    SocketAddr::V6(peer) => peer.scope_id(),
                    SocketAddr::V4(_) => 0,
                };
                self.video_proxy = ProxyOffer::from_params(&params, scope).map(Arc::new);
                match &self.video_proxy {
                    Some(offer) => {
                        println!("[cpStack] the phone offers a proxy at {}", offer.address())
                    }
                    None => println!("[cpStack] setProxyParameters without a usable proxy"),
                }
            }
            "" => {
                let stream = req
                    .headers
                    .get("x-apple-streamid")
                    .map_or("none", String::as_str);
                let data = params
                    .get("data")
                    .and_then(Value::as_data)
                    .unwrap_or_default();
                self.on_stream_payload(stream, data).await;
            }
            other => println!("[cpStack] unhandled command '{other}' (ack 200) {body:?}"),
        }
        status(200)
    }

    pub(super) fn modes_changed(&mut self, params: &Value) {
        let Some(states) = params.get("appStates").and_then(Value::as_array) else {
            return;
        };
        let mut in_call = self.call_active;
        for s in states {
            if int(s.get("appStateID")) == 2 && s.get("entity").is_some() {
                in_call = int(s.get("entity")) == 1;
            }
        }
        self.call_active = in_call;
        let mut active = self.speech_active;
        for s in states {
            if int(s.get("appStateID")) != 1 {
                continue;
            }
            let Some(mode) = s.get("speechMode") else {
                continue;
            };
            let mode = number(Some(mode));
            active = !in_call && (mode == 1.0 || mode == 2.0);
        }
        if in_call {
            active = false;
        }
        if active != self.speech_active {
            self.speech_active = active;
            println!(
                "[cpStack] Siri speech {}",
                if active { "active" } else { "done" }
            );
            self.emit(StackEvent::SpeechActive(active));
        }
    }

    pub(super) async fn on_cmd(&mut self, cmd: StackCmd) {
        match cmd {
            StackCmd::Touches(touches) => {
                if self.event.is_none() {
                    return;
                }
                let (w, h) = {
                    let cfg = self.ctx.config.borrow();
                    (
                        cfg.info.main.width_pixels as f64,
                        cfg.info.main.height_pixels as f64,
                    )
                };
                let contacts: Vec<Contact> = touches
                    .iter()
                    .map(|t| Contact {
                        x: t.x * w,
                        y: t.y * h,
                        down: t.down,
                    })
                    .collect();
                let report = hid::touch_report(&contacts);
                self.send_hid(hid::TOUCH_HID_UID, &report).await;
            }
            StackCmd::Knob { state, momentary } => {
                self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(state))
                    .await;
                if momentary {
                    self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(KnobState::default()))
                        .await;
                }
            }
            StackCmd::History(entries) => {
                if let Some(state) = &mut self.vehicle_state {
                    let answers = state.set_history(entries);
                    self.to_vehicle_streams(answers);
                }
            }
            StackCmd::Notification(notice) => match notice {
                Some(notice) => {
                    self.notice = Some(notice);
                    self.show_notice();
                }
                None => self.close_notice(),
            },
            StackCmd::KnobSelect(down) => {
                if down && self.notice_shown {
                    self.close_notice();
                    self.emit(StackEvent::NotificationClosed);
                    self.select_closed_notice = true;
                    return;
                }
                if !down && std::mem::take(&mut self.select_closed_notice) {
                    return;
                }
                let state = KnobState {
                    select: down,
                    ..Default::default()
                };
                self.send_hid(hid::KNOB_HID_UID, &hid::knob_report(state))
                    .await;
            }
            StackCmd::Media(index) => {
                self.send_hid(hid::MEDIA_HID_UID, &hid::media_report(index))
                    .await;
                self.send_hid(hid::MEDIA_HID_UID, &hid::media_report(0))
                    .await;
            }
            StackCmd::Telephony(index) => {
                self.send_hid(hid::TELEPHONY_HID_UID, &hid::telephony_report(index))
                    .await;
                self.send_hid(hid::TELEPHONY_HID_UID, &hid::telephony_report(0))
                    .await;
            }
            StackCmd::ClusterLayout(layout) => {
                let answers = self.ui_sync_state.select(layout);
                self.to_ui_sync(answers);
            }
            StackCmd::NextClusterContent => {
                if self.ctx.config.borrow().cluster_content.is_none() {
                    return;
                }
                println!("[cpStack] cluster knob turns the phone's centre zone");
                let turn = KnobState {
                    wheel: 1.0,
                    ..KnobState::default()
                };
                self.send_hid(hid::CLUSTER_KNOB_HID_UID, &hid::knob_report(turn))
                    .await;
                let rest = hid::knob_report(KnobState::default());
                self.send_hid(hid::CLUSTER_KNOB_HID_UID, &rest).await;
            }
            StackCmd::Siri => {
                if self.event.is_none() {
                    println!("[cpStack] invokeSiri: no active event connection, ignoring");
                    return;
                }
                println!("[cpStack] invokeSiri: requestSiri buttonDown+buttonUp (click)");
                for action in [2, 3] {
                    self.send_event_command(&dict([
                        ("type", Value::String("requestSiri".into())),
                        ("params", dict([("siriAction", Value::Int(action))])),
                    ]))
                    .await;
                }
            }
            StackCmd::RequestUi(url) => {
                if self.event.is_none() {
                    println!("[cpStack] requestUI: no active event connection, ignoring");
                    return;
                }
                println!("[cpStack] requestUI({url})");
                self.send_event_command(&dict([
                    ("type", Value::String("requestUI".into())),
                    ("params", dict([("url", Value::String(url))])),
                ]))
                .await;
            }
            StackCmd::NightMode(night) => {
                self.night_mode = Some(night);
                self.send_night_mode(night).await;
                if let Some(cluster) = self.cluster_night_mode {
                    self.send_cluster_appearance(cluster).await;
                }
            }
            StackCmd::ClusterNightMode(night) => {
                self.cluster_night_mode = Some(night);
                self.send_cluster_appearance(night).await;
                let answers = self.ui_sync_state.set_dark(night);
                self.to_ui_sync(answers);
            }
            StackCmd::LimitedUi(limited) => {
                self.limited_ui = Some(limited);
                self.send_limited_ui(limited).await;
            }
            StackCmd::VideoAllowed(allowed) => self.set_video_allowed(allowed).await,
            StackCmd::Keyframe => {
                if self.event.is_none() || !self.main_stream_ready {
                    println!("[cpStack] forceMainKeyframe skipped (stream not ready)");
                } else {
                    println!("[cpStack] forceKeyFrame -> main");
                    self.force_keyframe(&self.main_uuid()).await;
                }
                if self.event.is_none() || !self.main_stream_ready || !self.cluster_want {
                    println!(
                        "[cpStack] forceClusterKeyframe skipped (ready={} wantActive={})",
                        self.event.is_some() && self.main_stream_ready,
                        self.cluster_want
                    );
                } else {
                    println!("[cpStack] forceKeyFrame -> cluster");
                    self.force_keyframe(&self.alt_uuid()).await;
                }
            }
            StackCmd::ClusterActive(active) => {
                if self.ctx.config.borrow().info.cluster.is_none() {
                    return;
                }
                self.cluster_want = active;
                if self.event.is_none() || !self.main_stream_ready {
                    return;
                }
                if active {
                    self.activate_cluster_stream().await;
                } else {
                    self.send_event_command(&dict([
                        ("type", Value::String("stopUI".into())),
                        ("params", dict([("uuid", Value::String(self.alt_uuid()))])),
                    ]))
                    .await;
                }
            }
            StackCmd::VideoActive(active) => {
                self.video_active = active;
                for receiver in [self.screen, self.cluster_screen].into_iter().flatten() {
                    self.ctx.media.set_screen_active(receiver, active);
                }
            }
            StackCmd::AudioActive(active) => {
                self.audio_active = active;
                for a in &self.audio {
                    self.ctx.media.set_audio_active(a.host_stream, active);
                }
            }
            StackCmd::StreamVolume {
                kind,
                level,
                ramp_ms,
            } => {
                for a in self.audio.iter().filter(|a| a.profile.kind == kind) {
                    self.ctx
                        .media
                        .set_audio_volume(a.host_stream, level, ramp_ms);
                }
            }
            StackCmd::PhoneBtMac(mac) => self.phone_bt_mac = mac,
            StackCmd::Climate(climate) => {
                let Some(state) = &mut self.vehicle_state else {
                    return;
                };
                let answers = state.set_climate(&climate);
                self.to_vehicle_streams(answers);
                self.report_climate();
            }
            StackCmd::Gauges(gauges) => {
                let Some(state) = &mut self.vehicle_state else {
                    return;
                };
                let answers = state.set_gauges(&gauges);
                self.volume_reported = Some(state.volumes());
                self.to_vehicle_streams(answers);
            }
            StackCmd::Radio(radio) => {
                let Some(state) = &mut self.vehicle_state else {
                    return;
                };
                let answers = state.set_radio(&radio);
                self.to_vehicle_streams(answers);
            }
            StackCmd::Stop => {}
        }
    }
}
