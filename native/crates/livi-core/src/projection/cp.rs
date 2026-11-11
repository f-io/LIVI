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
                let ids = Ids {
                    wifi_mac: opt(&wifi_mac),
                    ip: opt(&ip),
                    ..Default::default()
                };
                self.devices.registry.note_wifi(&ids, connected);
                self.publish();
            }
            CpEvent::HelperPresence(HelperPresence::Device { ids, name }) => {
                let live = self.by_identity(&ids);
                let wired = live.map_or(ids.usb_udid.is_some(), |i| {
                    self.sessions[i].transport == Transport::Usb
                });
                let seen = Seen {
                    ids,
                    name,
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
            CpEvent::HostUiRequested { id } => {
                println!("[core] CarPlay asks for the car's own UI");
                if self.is_active(Link::Cp(id)) {
                    self.host_ui();
                }
            }
            CpEvent::Video { id, shown } => {
                if let Some(entry) = self.by_link(Link::Cp(id)) {
                    entry.video_playback = shown.map(|v| VideoPlayback {
                        playing: v.playing,
                        position_ms: v.position * 1000.0,
                        duration_ms: v.duration.map(|d| d * 1000.0),
                    });
                    self.publish();
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
            CpEvent::Metadata { id, event } => self.metadata(Link::Cp(id), event),
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
                if let (Some(radio), Some(ask)) =
                    (&self.radio, crate::projection::cp_radio::ask(command))
                {
                    let _ = radio.send(ask);
                }
            }
            CpEvent::VideoCodec { .. } => {}
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
                    ids: Ids {
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
                    .map(|s| s.device.clone())
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

    pub(super) fn metadata(&mut self, link: Link, event: HelperKind) {
        let Some(entry) = self.by_link(link) else {
            return;
        };
        match event {
            HelperKind::Navigation(patch) => patch.apply_to(&mut entry.navigation),
            HelperKind::NowPlaying(patch) => patch.apply_to(&mut entry.now_playing),
            _ => return,
        }
        let beside_airplay = self.active().is_some_and(|a| matches!(a.link, Link::Ap(_)));
        if self.is_active(link) || beside_airplay {
            self.publish();
        }
    }
}
