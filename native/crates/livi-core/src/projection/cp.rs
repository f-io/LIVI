// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn on_event(&mut self, ev: CpEvent) {
        match ev {
            CpEvent::Connected {
                id,
                wired,
                controller_id,
            } => {
                let ids = Ids {
                    controller_id,
                    ..Default::default()
                };
                let index = self.upsert(Link::Cp(id), transport(wired), &ids);
                self.auto_activate(index);
                if let Some(layout) = self.phone_layout() {
                    self.cp.stack(id, StackCmd::ClusterLayout(layout));
                }
            }
            CpEvent::Presence {
                id,
                wired,
                presence,
            } => self.presence(id, wired, presence),
            CpEvent::Disconnected { id } => self.close_by_link(Link::Cp(id)),
            CpEvent::AudioActive {
                id,
                profile,
                active,
            } => {
                println!(
                    "[core] CarPlay audio {} {}",
                    profile.label,
                    if active { "started" } else { "stopped" }
                );
                if active && self.is_active(Link::Cp(id)) {
                    self.push_level(profile.kind, 0);
                }
                if self.is_active(Link::Cp(id)) {
                    self.activity(|a| a.stream(profile.kind, active));
                }
                if profile.kind == AudioKind::Call && self.is_active(Link::Cp(id)) {
                    self.attend(Attention::Call, active);
                }
            }
            CpEvent::Duck {
                id,
                level,
                duration_ms,
            } => self.on_duck(Link::Cp(id), level, duration_ms),
            CpEvent::HelperPresence(HelperPresence::Wifi {
                wifi_mac,
                ip,
                connected,
            }) => {
                let ids = devices::Ids {
                    wifi_mac: opt(&wifi_mac),
                    ip: opt(&ip),
                    ..Default::default()
                };
                self.devices.registry.note_wifi(&ids, connected);
                if !connected {
                    self.close_on_transport(&session_ids(&ids), Transport::Wifi);
                }
                self.publish();
            }
            CpEvent::HelperPresence(HelperPresence::Device { ids }) => {
                let found = devices::Ids {
                    bt_mac: ids.bt_mac,
                    usb_udid: ids.usb_udid,
                    ip: ids.ip,
                    ..Default::default()
                };
                let live = self.by_identity(&session_ids(&found));
                let wired = live.map_or(found.usb_udid.is_some(), |i| {
                    self.sessions[i].transport == Transport::Usb
                });
                let seen = Seen {
                    ids: found,
                    name: ids.name,
                    protocol: Some(Protocol::Carplay),
                    wired,
                    ..Default::default()
                };
                self.devices.registry.note_device(&seen);
                self.publish();
            }
            CpEvent::HelperConnected => {
                let targets = self.targets();
                self.devices.page(targets, true);
            }
            CpEvent::HelperPresence(HelperPresence::DeviceGone { usb_udid }) => {
                let ids = Ids {
                    usb_udid: opt(&usb_udid),
                    ..Default::default()
                };
                self.close_on_transport(&ids, Transport::Usb);
            }
            CpEvent::HostUiRequested { id } => {
                println!("[core] CarPlay asks for the car's own UI");
                if self.is_active(Link::Cp(id)) {
                    self.host_ui();
                }
            }
            CpEvent::SpeechActive { id, active } => {
                println!(
                    "[core] CarPlay Siri {}",
                    if active { "active" } else { "done" }
                );
                if self.is_active(Link::Cp(id)) {
                    self.attend(Attention::VoiceAssistant, active);
                }
            }
            CpEvent::Call { id, phase } => {
                println!("[core] CarPlay call {phase:?}");
                if self.is_active(Link::Cp(id)) {
                    self.attend(Attention::Call, phase != CallPhase::Ended);
                    self.activity(|a| a.phone = phase != CallPhase::Ended);
                }
            }
            CpEvent::Metadata { id, event } => self.metadata(Link::Cp(id), &event),
            CpEvent::Spawned { .. } => {
                self.cp_telemetry.hydrate(&self.telemetry_seen, &self.cp);
                self.stream_volumes_to_phone();
            }
            CpEvent::Climate { mut climate, .. } => {
                climate.insert(SOURCE.into(), FROM_CARPLAY.into());
                let _ = self.telemetry.send(serde_json::json!({ CLIMATE: climate }));
            }
            CpEvent::NotificationClosed { .. } => {
                let _ = self
                    .telemetry
                    .send(serde_json::json!({ NOTIFICATION: { "open": false } }));
            }
            CpEvent::Volume { name, level, .. } => {
                let to = if name == "volume" {
                    &self.telemetry
                } else {
                    &self.settings
                };
                let _ = to.send(serde_json::json!({ name: level }));
            }
            CpEvent::Cluster { id, choice } => {
                if self.is_active(Link::Cp(id)) && self.follow_phone(choice) {
                    self.keep_cluster();
                    self.publish();
                }
            }
            CpEvent::Radio { command, .. } => {
                if let (Some(dab), Some(ask)) = (&self.dab, crate::cp_radio::ask(command)) {
                    let _ = dab.send(ask);
                }
            }
            CpEvent::VideoCodec { id, cluster, .. } => {
                if !cluster {
                    self.video_up(Link::Cp(id));
                }
            }
            CpEvent::PhoneUtcOffset { minutes } => {
                tokio::spawn(crate::gnss::timezone::apply_phone_offset(minutes));
            }
        }
    }

    pub(super) fn presence(&mut self, id: SessionId, wired: bool, presence: Presence) {
        match presence {
            Presence::Device {
                bt_mac,
                wifi_mac,
                ip,
                usb_udid,
                name,
                model,
            } => {
                let ids = Ids {
                    bt_mac: opt(&bt_mac),
                    wifi_mac: if wired { None } else { opt(&wifi_mac) },
                    usb_udid: opt(&usb_udid),
                    ip: opt(&ip),
                    ..Default::default()
                };
                let seen = Seen {
                    ids: devices::Ids {
                        bt_mac: ids.bt_mac.clone(),
                        wifi_mac: ids.wifi_mac.clone(),
                        usb_udid: ids.usb_udid.clone(),
                        ip: ids.ip.clone(),
                        ..Default::default()
                    },
                    name: opt(&name),
                    model: opt(&model),
                    protocol: Some(Protocol::Carplay),
                    wired,
                };
                self.devices.registry.note_device(&seen);
                if let Some(i) = self.by_identity(&ids)
                    && self.sessions[i].link != Link::Cp(id)
                {
                    let placeholder = self.sessions[i].link;
                    self.sessions[i].link = Link::Cp(id);
                    self.tell(placeholder, Do::Close);
                    self.changed();
                }
                let index = self.upsert(Link::Cp(id), transport(wired), &ids);
                self.auto_activate(index);
            }
            Presence::Active { .. } => {
                if let Some(index) = self.by_link(Link::Cp(id)).map(|s| s.index) {
                    self.auto_activate(index);
                }
            }
            Presence::Status {
                battery_level,
                battery_charging,
                signal_strength,
                carrier_name,
            } => {
                let Some(ids) = self
                    .sessions
                    .iter()
                    .find(|s| s.link == Link::Cp(id))
                    .map(|s| registry_ids(&s.device))
                else {
                    return;
                };
                let status = devices::Status {
                    battery_level,
                    battery_charging,
                    signal_strength,
                    carrier_name,
                };
                self.devices.registry.note_status(&ids, &status);
                self.publish();
            }
        }
    }

    pub(super) fn metadata(&mut self, link: Link, event: &Value) {
        let Some(entry) = self.by_link(link) else {
            return;
        };
        let text = |key: &str| event.get(key).and_then(Value::as_str).map(str::to_string);
        let num = |key: &str| event.get(key).and_then(Value::as_f64);
        let code = |key: &str| num(key).map(|v| v as u32);
        match event.get("type").and_then(Value::as_str) {
            Some("navigation") => {
                let nav = &mut entry.navigation;
                if let Some(status) = num("status") {
                    if status == 0.0 {
                        *nav = Navigation::default();
                    }
                    nav.active = Some(status != 0.0);
                }
                for (field, key) in [
                    (&mut nav.road_name, "roadName"),
                    (&mut nav.destination_name, "destinationName"),
                    (
                        &mut nav.distance_to_destination_display,
                        "distanceToDestinationDisplay",
                    ),
                    (&mut nav.remain_distance_display, "remainDistanceDisplay"),
                    (&mut nav.app_name, "appName"),
                ] {
                    if let Some(v) = text(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut nav.time_to_destination, "timeToDestination"),
                    (&mut nav.distance_to_destination, "distanceToDestination"),
                    (&mut nav.remain_distance, "remainDistance"),
                ] {
                    if let Some(v) = num(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut nav.order_type, "orderType"),
                    (
                        &mut nav.distance_to_destination_units,
                        "distanceToDestinationUnits",
                    ),
                    (&mut nav.remain_distance_units, "remainDistanceUnits"),
                    (&mut nav.maneuver_count, "maneuverCount"),
                    (&mut nav.lane_guidance_index, "laneGuidanceIndex"),
                    (&mut nav.lane_guidance_count, "laneGuidanceCount"),
                ] {
                    if let Some(v) = code(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut nav.visible_in_app, "visibleInApp"),
                    (&mut nav.lane_guidance_shown, "laneGuidanceShown"),
                    (
                        &mut nav.source_supports_route_guidance,
                        "sourceSupportsRouteGuidance",
                    ),
                ] {
                    if let Some(v) = event.get(key).and_then(Value::as_bool) {
                        *field = Some(v);
                    }
                }
                if let Some(v) = num("destinationTimeZoneOffsetMinutes") {
                    nav.destination_time_zone_offset_minutes = Some(v as i32);
                }
                for (field, key) in [
                    (&mut nav.maneuver_type, "maneuverType"),
                    (&mut nav.turn_side, "turnSide"),
                    (&mut nav.junction_type, "junctionType"),
                ] {
                    if event.get(key).is_some() {
                        *field = code(key);
                    }
                }
                if event.get("turnAngle").is_some() {
                    nav.turn_angle = num("turnAngle").map(|v| v as i32);
                }
                if event.get("afterRoadName").is_some() {
                    nav.after_road_name = text("afterRoadName");
                }
                if let Some(v) = num("etaEpoch").filter(|v| *v > 0.0) {
                    nav.eta = Some(v);
                }
                if let Some(Ok(v)) = event.get("maneuvers").cloned().map(serde_json::from_value) {
                    nav.maneuvers = v;
                }
                if let Some(Ok(v)) = event
                    .get("currentManeuvers")
                    .cloned()
                    .map(serde_json::from_value)
                {
                    nav.current_maneuvers = v;
                }
                if let Some(Ok(v)) = event
                    .get("laneGuidance")
                    .cloned()
                    .map(serde_json::from_value)
                {
                    nav.lane_guidance = v;
                }
            }
            Some("nowplaying") => {
                let np = &mut entry.now_playing;
                for (field, key) in [
                    (&mut np.title, "title"),
                    (&mut np.artist, "artist"),
                    (&mut np.album, "album"),
                    (&mut np.app, "appName"),
                ] {
                    if let Some(v) = text(key) {
                        *field = Some(v);
                    }
                }
                for (field, key) in [
                    (&mut np.duration_ms, "durationMs"),
                    (&mut np.elapsed_ms, "elapsedMs"),
                ] {
                    if let Some(v) = num(key) {
                        *field = Some(v);
                    }
                }
                if let Some(v) = num("playing") {
                    np.playing = Some(v == 1.0);
                }
            }
            Some("albumart") => match text("dataB64") {
                Some(b64) if !b64.is_empty() => entry.now_playing.artwork = Some(b64),
                _ => return,
            },
            _ => return,
        }
        if self.is_active(link) {
            self.publish();
        }
    }
}
