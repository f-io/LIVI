// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) async fn handle_setup(&mut self, req: &Request) -> Response {
        let body = match bplist::decode(&req.body) {
            Ok(v) => v,
            Err(e) => {
                println!("[cpStack] SETUP body is not a plist: {e}");
                return status(400);
            }
        };
        if let Some(streams) = body.get("streams").and_then(Value::as_array) {
            let mut out = Vec::new();
            for sd in streams {
                let kind = int(sd.get("type"));
                match kind {
                    STREAM_MAIN_SCREEN | STREAM_ALT_SCREEN => {
                        let port = self.setup_screen(sd, kind == STREAM_ALT_SCREEN).await;
                        match port {
                            Ok(port) => out.push(dict([
                                ("type", Value::Int(kind)),
                                ("dataPort", Value::Int(u64::from(port))),
                            ])),
                            Err(e) => return self.setup_failed(req, &e),
                        }
                    }
                    STREAM_MAIN_AUDIO | STREAM_ALT_AUDIO | STREAM_MAIN_HIGH_AUDIO => {
                        println!("[cpStack] SETUP audio stream type {kind}");
                        match self.setup_audio(sd, kind).await {
                            Ok(v) => out.push(v),
                            Err(e) => return self.setup_failed(req, &e),
                        }
                    }
                    STREAM_DATA => match self.setup_data_stream(sd) {
                        Ok(Some(v)) => out.push(v),
                        Ok(None) => {}
                        Err(e) => return self.setup_failed(req, &e),
                    },
                    other => println!("[cpStack]   SETUP stream type {other} not handled yet"),
                }
            }
            return plist_response(&dict([("streams", Value::Array(out))]));
        }

        let text = |k: &str| {
            body.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let (name, device_id, model) = (text("name"), text("deviceID"), text("model"));
        let wifi_mac = text("macAddress").to_lowercase();
        if !device_id.is_empty() {
            self.device_bt_mac = device_id.clone();
        }
        if !name.is_empty() || !device_id.is_empty() || !wifi_mac.is_empty() {
            self.emit(StackEvent::DeviceInfo {
                name,
                device_id,
                wifi_mac,
                model,
            });
        }
        let offered: Vec<&str> = body
            .get("features")
            .and_then(Value::as_array)
            .map(|f| f.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !offered.is_empty() {
            println!("[cpStack] phone offers {}", offered.join(", "));
        }
        if offered.contains(&video_playback::FEATURE) && self.ctx.config.borrow().hls_video {
            self.video_playback = true;
            if let Some(root) = &self.ctx.video_capture {
                let capture = video_playback::Capture::new(root);
                println!("[cpStack] video payloads go to {}", capture.dir().display());
                self.video_capture = Some(capture);
            }
        }
        self.offered_vehicle_state = offered.contains(&vehicle_state::FEATURE);
        self.ui_sync_on = offered.contains(&ui_sync::FEATURE)
            && self.ctx.config.borrow().info.theme_displays.is_some();

        let mut timing = match TimingSync::listen() {
            Ok(t) => t,
            Err(e) => return self.setup_failed(req, &e.to_string()),
        };
        let timing_port = timing.port();
        let peer_timing = int(body.get("timingPort"));
        if let Ok(port) = u16::try_from(peer_timing)
            && port > 0
        {
            let mut peer = self.peer;
            peer.set_port(port);
            timing.start(peer);
        }
        self.timing = Some(timing);
        let event_port = match net::tcp_listener() {
            Ok(l) => {
                let port = net::local_port(l.local_addr());
                self.event_listener = Some(l);
                port
            }
            Err(e) => return self.setup_failed(req, &e.to_string()),
        };
        let mut keep_alive_port = 0;
        if truthy(body.get("keepAliveLowPower")) {
            match KeepAlive::listen() {
                Ok(k) => {
                    keep_alive_port = k.port();
                    self.keep_alive = Some(k);
                }
                Err(e) => return self.setup_failed(req, &e.to_string()),
            }
        }
        println!(
            "[cpStack] SETUP session (timingPort={timing_port}, eventPort={event_port}, keepAlivePort={keep_alive_port})"
        );
        let cfg = self.ctx.config.borrow().info.clone();
        let mut features = Vec::new();
        if cfg.hevc {
            features.push(Value::String("hevc".into()));
        }
        features.push(Value::String("iAPChannel".into()));
        features.push(Value::String("viewAreas".into()));
        if cfg.cluster.is_some() {
            features.push(Value::String("altScreen".into()));
        }
        if self.video_playback {
            features.push(Value::String(video_playback::FEATURE.into()));
        }
        if self.offered_vehicle_state {
            features.push(Value::String(vehicle_state::FEATURE.into()));
        }
        if self.ui_sync_on {
            features.push(Value::String(ui_sync::FEATURE.into()));
        }
        let mut resp = vec![
            ("timingPort".to_string(), Value::Int(u64::from(timing_port))),
            ("eventPort".to_string(), Value::Int(u64::from(event_port))),
        ];
        if keep_alive_port > 0 {
            resp.push((
                "keepAlivePort".to_string(),
                Value::Int(u64::from(keep_alive_port)),
            ));
        }
        resp.push(("enabledFeatures".to_string(), Value::Array(features)));
        plist_response(&Value::Dict(resp))
    }

    pub(super) fn setup_failed(&self, req: &Request, e: &str) -> Response {
        println!(
            "[cpStack] handler error for {} {}: {e}",
            req.method, req.path
        );
        status(500)
    }

    pub(super) async fn setup_screen(&mut self, sd: &Value, cluster: bool) -> Result<u16, String> {
        let shared = self
            .pair_verify
            .shared_secret()
            .ok_or("screen SETUP arrived before pair-verify")?;
        let id = id_text(sd.get("streamConnectionID"));
        let key = stream_key(&shared, &id, "DataStream-Output-Encryption-Key");
        let codec = if self.ctx.config.borrow().info.hevc {
            ScreenCodec::H265
        } else {
            ScreenCodec::H264
        };
        let (port, receiver) = self.ctx.media.open_screen(cluster, key).await;
        if self.video_active {
            self.ctx.media.set_screen_active(receiver, true);
        }
        if cluster {
            self.cluster_screen = Some(receiver);
            if !self.cluster_codec_emitted {
                self.cluster_codec_emitted = true;
                self.emit(StackEvent::VideoCodec {
                    cluster: true,
                    codec,
                });
            }
        } else {
            self.screen = Some(receiver);
            if !self.codec_emitted {
                self.codec_emitted = true;
                self.emit(StackEvent::VideoCodec {
                    cluster: false,
                    codec,
                });
            }
            if !self.main_stream_ready {
                self.main_stream_ready = true;
                if self.cluster_want {
                    self.activate_cluster_stream().await;
                }
            }
            self.emit(StackEvent::MainScreenReady);
        }
        println!(
            "[cpStack] SETUP screen NATIVE ({}, dataPort={port}, id={id}, codec={codec:?})",
            if cluster { "cluster" } else { "main" }
        );
        Ok(port)
    }

    pub(super) async fn setup_audio(&mut self, sd: &Value, kind: u64) -> Result<Value, String> {
        let shared = self
            .pair_verify
            .shared_secret()
            .ok_or("audio SETUP arrived before pair-verify")?;
        let connection_id = sd
            .get("streamConnectionID")
            .cloned()
            .unwrap_or(Value::Int(0));
        let id = id_text(sd.get("streamConnectionID"));
        let latency_ms = int(sd.get("audioLatencyMs")) as u32;
        let key = stream_key(&shared, &id, "DataStream-Output-Encryption-Key");
        let audio_type = sd
            .get("audioType")
            .map_or_else(|| "media".to_string(), text);
        let profile = audio_profile(&audio_type);
        let fmt = int(sd.get("audioFormat"));
        let is_aac = fmt & (AAC_LC_44K_STEREO | AAC_LC_48K_STEREO) != 0;
        let is_opus = fmt & OPUS_MONO != 0;
        let (sample_rate, channels) = if is_opus {
            (48000, 1)
        } else if is_aac {
            (
                if fmt & AAC_LC_48K_STEREO != 0 {
                    48000
                } else {
                    44100
                },
                2,
            )
        } else {
            pcm_format(fmt).unwrap_or((44100, 2))
        };
        let codec = if is_aac {
            AudioCodec::AacLc
        } else if is_opus {
            AudioCodec::Opus
        } else {
            AudioCodec::Pcm
        };
        let (device, input_device) = {
            let cfg = self.ctx.config.borrow();
            (cfg.audio_device.clone(), cfg.audio_input_device.clone())
        };
        let request = AudioRequest {
            codec,
            payload_type: kind as u8,
            clock_rate: if is_opus { 48000 } else { sample_rate },
            channels,
            latency_ms: if latency_ms > 0 { latency_ms } else { 1000 },
            realtime: profile.kind != AudioKind::Media,
            device,
        };
        let opened = self.ctx.media.open_audio(key, request).await;
        if self.audio_active {
            self.ctx.media.set_audio_active(opened.id, true);
        }

        let phone_mic_port = if kind == STREAM_MAIN_AUDIO {
            int(sd.get("dataPort"))
        } else {
            0
        };
        let opus_rate = if fmt & OPUS_48K != 0 {
            48000
        } else if fmt & OPUS_24K != 0 {
            24000
        } else if fmt & OPUS_16K != 0 {
            16000
        } else {
            24000
        };
        let mic_rate = if is_opus { opus_rate } else { sample_rate };
        let mic_channels = if is_opus { 1 } else { channels };
        let frames = int(sd.get("framesPerPacket"));
        let frame_ms = if frames > 0 {
            (frames as f64 / f64::from(mic_rate) * 1000.0 + 0.5).floor() as u32
        } else {
            20
        };
        let bitrate = if mic_rate <= 24000 {
            48000
        } else if mic_rate <= 32000 {
            64000
        } else {
            96000
        };
        let mic_plan = u16::try_from(phone_mic_port)
            .ok()
            .filter(|p| *p > 0)
            .map(|port| {
                (
                    stream_key(&shared, &id, "DataStream-Input-Encryption-Key"),
                    MicRequest {
                        opus: is_opus,
                        payload_type: kind as u8,
                        sample_rate: mic_rate,
                        channels: mic_channels,
                        bitrate,
                        frame_ms,
                        port,
                        phone: net::route_of(&self.peer),
                        device: input_device,
                    },
                )
            });
        println!(
            "[cpStack] SETUP audio (type {kind}, audioType={audio_type}, format=0x{fmt:x}, codec={codec:?}, audioLatencyMs={latency_ms}, dataPort={}, controlPort={}, id={id})",
            opened.data_port, opened.control_port
        );
        self.audio.push(AudioStreamState {
            stream_type: kind,
            sample_rate,
            connection_id: connection_id.clone(),
            host_stream: opened.id,
            mic: None,
            origin: None,
            playout_latency_ms: latency_ms,
            profile,
            mic_plan,
        });
        Ok(dict([
            ("type", Value::Int(kind)),
            ("dataPort", Value::Int(u64::from(opened.data_port))),
            ("controlPort", Value::Int(u64::from(opened.control_port))),
            ("streamConnectionID", connection_id),
        ]))
    }
}
