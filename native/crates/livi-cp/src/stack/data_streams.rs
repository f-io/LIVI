// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) fn setup_vehicle_stream(&mut self, lane: Lane, sd: &Value) -> Result<Value, String> {
        let shared = self
            .pair_verify
            .shared_secret()
            .ok_or("vehicle state DataStream SETUP arrived before pair-verify")?;
        let into_session = self
            .vehicle_in
            .get_or_insert_with(|| {
                let (tx, rx) = mpsc::unbounded_channel();
                self.vehicle_out = Some(rx);
                tx
            })
            .clone();
        let deliver = Arc::new(move |message: Vec<u8>| {
            let _ = into_session.send((lane, message));
        });
        let name = match lane {
            Lane::High => "vehicle state (high priority)",
            Lane::Normal => "vehicle state",
        };
        let seed = id_text(sd.get("seed"));
        let (tunnel, to_phone) =
            IapTunnel::listen_duplex(name, &shared, &seed, deliver).map_err(|e| e.to_string())?;
        let port = tunnel.port();
        self.vehicle_streams.retain(|(l, ..)| *l != lane);
        self.vehicle_streams.push((lane, tunnel, to_phone));
        self.last_stream_id += 1;
        let id = self.last_stream_id;
        println!("[cpStack] SETUP {name} (type 130, streamID={id}, dataPort={port})");
        Ok(dict([
            ("type", Value::Int(STREAM_DATA)),
            ("streamID", Value::Int(id)),
            ("dataPort", Value::Int(u64::from(port))),
        ]))
    }

    pub(super) fn on_ui_sync(&mut self, message: &[u8]) {
        let answers = self.ui_sync_state.handle(message);
        self.to_ui_sync(answers);
        for choice in self.ui_sync_state.take_choices() {
            self.emit(StackEvent::Cluster(choice));
        }
    }

    pub(super) fn to_ui_sync(&self, messages: Vec<Vec<u8>>) {
        if let Some((_, to_phone)) = &self.ui_sync {
            for message in messages {
                let _ = to_phone.send(message);
            }
        }
    }

    pub(super) fn on_vehicle_message(&mut self, lane: Lane, message: &[u8]) {
        let Some(state) = &mut self.vehicle_state else {
            return;
        };
        let answers = state.handle(lane, message);
        let commands = state.take_radio_commands();
        let hidden = state.take_hidden_by_phone();
        let open = state.notice_open();
        if state.take_zone_ready() {
            self.notice_ready = false;
            self.notice_ready_at = Some(tokio::time::Instant::now() + NOTICE_DELAY);
        }
        self.to_vehicle_streams(answers);
        if self.notice_shown && !open {
            println!("[cpStack] the phone closed the notification");
            self.notice_shown = false;
            self.notice = None;
            self.emit(StackEvent::NotificationClosed);
        }
        self.report_climate();
        self.report_volume();
        for c in commands {
            self.emit(StackEvent::Radio(c));
        }
        for id in hidden {
            self.emit(StackEvent::HistoryHidden(id));
        }
    }

    pub(super) fn report_volume(&mut self) {
        let Some(levels) = self.vehicle_state.as_ref().map(VehicleState::volumes) else {
            return;
        };
        let seen = self.volume_reported.replace(levels.clone());
        let Some(seen) = seen else { return };
        for (name, level) in levels {
            if !seen.contains(&(name, level)) {
                self.emit(StackEvent::Volume { name, level });
            }
        }
    }

    pub(super) fn report_climate(&mut self) {
        let Some(climate) = self.vehicle_state.as_ref().and_then(VehicleState::climate) else {
            return;
        };
        if self.climate_reported.as_ref() != Some(&climate) {
            self.climate_reported = Some(climate.clone());
            self.emit(StackEvent::Climate(climate));
        }
    }

    pub(super) fn show_notice(&mut self) {
        let (Some(notice), Some(state)) = (&self.notice, &mut self.vehicle_state) else {
            return;
        };
        if !self.notice_ready {
            return;
        }
        let answers = state.show_notice(notice);
        self.notice_shown = !answers.is_empty();
        self.to_vehicle_streams(answers);
    }

    pub(super) fn close_notice(&mut self) {
        self.notice = None;
        self.notice_shown = false;
        let Some(state) = &mut self.vehicle_state else {
            return;
        };
        let answers = state.hide_notice();
        self.to_vehicle_streams(answers);
    }

    pub(super) fn to_vehicle_streams(&self, answers: Vec<(Lane, Vec<u8>)>) {
        for (lane, answer) in answers {
            let stream = self.vehicle_streams.iter().find(|(l, ..)| *l == lane);
            match stream.or(self.vehicle_streams.first()) {
                Some((_, _, to_phone)) => {
                    let _ = to_phone.send(answer);
                }
                None => println!("[cpVehicle] no stream left to answer on"),
            }
        }
    }

    pub(super) fn setup_data_stream(&mut self, sd: &Value) -> Result<Option<Value>, String> {
        let uuid = sd
            .get("clientTypeUUID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_uppercase();
        if uuid == video_playback::DATASTREAM_UUID && self.video_playback {
            self.last_stream_id += 1;
            let id = self.last_stream_id;
            let client = sd
                .get("clientUUID")
                .and_then(Value::as_str)
                .unwrap_or("none");
            println!("[cpStack] SETUP video playback (type 130, streamID={id}, client {client})");
            return Ok(Some(dict([
                ("type", Value::Int(STREAM_DATA)),
                ("streamID", Value::Int(id)),
            ])));
        }
        if uuid == video_playback::SETTINGS_DATASTREAM_UUID
            && self.video_playback
            && self.video_settings.is_none()
        {
            let shared = self
                .pair_verify
                .shared_secret()
                .ok_or("video settings DataStream SETUP arrived before pair-verify")?;
            let seed = id_text(sd.get("seed"));
            let (tx, rx) = mpsc::unbounded_channel();
            let tunnel = IapTunnel::listen_as("video settings", &shared, &seed, tx)
                .map_err(|e| e.to_string())?;
            let port = tunnel.port();
            self.video_settings = Some(tunnel);
            self.video_settings_out = Some(rx);
            self.last_stream_id += 1;
            let id = self.last_stream_id;
            println!("[cpStack] SETUP video settings (type 130, streamID={id}, dataPort={port})");
            return Ok(Some(dict([
                ("type", Value::Int(STREAM_DATA)),
                ("streamID", Value::Int(id)),
                ("dataPort", Value::Int(u64::from(port))),
            ])));
        }
        if uuid == ui_sync::DATASTREAM_UUID && self.ui_sync_on && self.ui_sync.is_none() {
            let shared = self
                .pair_verify
                .shared_secret()
                .ok_or("uiSync DataStream SETUP arrived before pair-verify")?;
            let seed = id_text(sd.get("seed"));
            let (tx, rx) = mpsc::unbounded_channel();
            let deliver = Arc::new(move |message: Vec<u8>| {
                let _ = tx.send(message);
            });
            let (tunnel, to_phone) = IapTunnel::listen_duplex("uiSync", &shared, &seed, deliver)
                .map_err(|e| e.to_string())?;
            let port = tunnel.port();
            self.ui_sync = Some((tunnel, to_phone));
            self.ui_sync_out = Some(rx);
            self.ui_sync_state.reopen();
            self.last_stream_id += 1;
            let id = self.last_stream_id;
            println!("[cpStack] SETUP uiSync (type 130, streamID={id}, dataPort={port})");
            return Ok(Some(dict([
                ("type", Value::Int(STREAM_DATA)),
                ("streamID", Value::Int(id)),
                ("dataPort", Value::Int(u64::from(port))),
            ])));
        }
        if uuid == video_playback::OVERLAY_DATASTREAM_UUID
            && self.video_playback
            && self.video_overlay.is_none()
        {
            let shared = self
                .pair_verify
                .shared_secret()
                .ok_or("video overlay DataStream SETUP arrived before pair-verify")?;
            let seed = id_text(sd.get("seed"));
            let (tx, rx) = mpsc::unbounded_channel();
            let tunnel = IapTunnel::listen_as("video overlay", &shared, &seed, tx)
                .map_err(|e| e.to_string())?;
            let port = tunnel.port();
            self.video_overlay = Some(tunnel);
            self.video_overlay_out = Some(rx);
            self.last_stream_id += 1;
            let id = self.last_stream_id;
            println!("[cpStack] SETUP video overlay (type 130, streamID={id}, dataPort={port})");
            return Ok(Some(dict([
                ("type", Value::Int(STREAM_DATA)),
                ("streamID", Value::Int(id)),
                ("dataPort", Value::Int(u64::from(port))),
            ])));
        }
        let lane = match uuid.as_str() {
            vehicle_state::HIGH_PRIORITY_DATASTREAM_UUID => Some(Lane::High),
            vehicle_state::DATASTREAM_UUID => Some(Lane::Normal),
            _ => None,
        };
        if let Some(lane) = lane.filter(|_| self.vehicle_state.is_some()) {
            return self.setup_vehicle_stream(lane, sd).map(Some);
        }
        if uuid != IAP_DATASTREAM_UUID {
            let what = match uuid.as_str() {
                "" => "(no uuid)",
                video_playback::DATASTREAM_UUID => "for video playback",
                video_playback::SETTINGS_DATASTREAM_UUID => "for video settings",
                video_playback::OVERLAY_DATASTREAM_UUID => "for video overlay",
                other => other,
            };
            println!("[cpStack]   DataStream {what} not handled yet");
            return Ok(None);
        }
        let shared = self
            .pair_verify
            .shared_secret()
            .ok_or("iAP DataStream SETUP arrived before pair-verify")?;
        let seed = id_text(sd.get("seed"));
        let (tx, rx) = mpsc::unbounded_channel();
        let tunnel = IapTunnel::listen(&shared, &seed, tx).map_err(|e| e.to_string())?;
        let port = tunnel.port();
        self.iap_tunnel = Some(tunnel);
        self.iap_out = Some(rx);
        println!("[cpStack] SETUP iAP tunnel (type 130, dataPort={port}, seed={seed})");
        Ok(Some(dict([
            ("type", Value::Int(STREAM_DATA)),
            ("streamID", Value::Int(1)),
            ("dataPort", Value::Int(u64::from(port))),
        ])))
    }

    pub(super) async fn open_iap_relay(&mut self) {
        if self.iap_relay.is_some() {
            return;
        }
        let mac = if self.device_bt_mac.is_empty() {
            self.phone_bt_mac.clone()
        } else {
            self.device_bt_mac.clone()
        };
        let cid = self.pair_verify.controller_id().unwrap_or("").to_string();
        println!(
            "[cpStack] opening iAP relay (cid={cid}, btMac={})",
            if mac.is_empty() { "unknown" } else { &mac }
        );
        match self.ctx.helper.open_tunnel(&cid, &mac).await {
            Ok(sock) => self.iap_relay = Some(sock.into_split()),
            Err(e) => println!("[cpStack] iAP relay error: {e}"),
        }
    }
}
