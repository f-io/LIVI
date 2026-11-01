// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn upsert(&mut self, link: Link, transport: Transport, device: &Ids) -> u32 {
        let by_identity = self.by_identity(device);
        let steals_wired = by_identity.is_some_and(|i| {
            let s = &self.sessions[i];
            s.link != link && s.transport == Transport::Usb && transport != Transport::Usb
        });
        let found = match by_identity {
            Some(i) if !steals_wired => Some(i),
            _ => self.sessions.iter().position(|s| s.link == link),
        };
        let mut superseded = None;
        let index = match found {
            Some(i) => {
                let s = &mut self.sessions[i];
                if s.link != link && transport == Transport::Usb && s.transport != Transport::Usb {
                    superseded = Some(s.link);
                    s.link = link;
                }
                s.transport = transport;
                s.device.merge(device);
                s.index
            }
            None => {
                let index = self.next_index;
                self.next_index += 1;
                let mut entry = Entry {
                    index,
                    link,
                    transport,
                    device: Ids::default(),
                    active: false,
                    video: false,
                    duck_level: 1.0,
                    duck_ramp_ms: RAMP_UP_MS,
                    now_playing: NowPlaying::default(),
                    navigation: Navigation::default(),
                };
                entry.device.merge(device);
                self.sessions.push(entry);
                index
            }
        };
        if let Some(old) = superseded {
            self.tell(old, Do::Close);
        }
        self.changed();
        index
    }

    pub(super) fn video_up(&mut self, link: Link) {
        let Some(entry) = self.by_link(link) else {
            return;
        };
        if entry.video {
            return;
        }
        entry.video = true;
        if self.is_active(link) {
            self.publish();
        }
    }

    pub(super) fn auto_activate(&mut self, index: u32) {
        if self.active().is_none() {
            self.activate(index);
        }
    }

    pub(super) fn activate(&mut self, index: u32) {
        let Some(target) = self.sessions.iter().position(|s| s.index == index) else {
            return;
        };
        if self.sessions[target].active {
            return;
        }
        let prev = self.sessions.iter().position(|s| s.active);
        let had_prev = prev.is_some();
        if let Some(p) = prev {
            self.sessions[p].active = false;
        }
        self.sessions[target].active = true;
        println!("[core] active session #{index}");
        self.on_active_changed(Some(target), had_prev);
        self.changed();
    }

    pub(super) fn activate_next(&mut self) {
        if self.sessions.len() <= 1 {
            return;
        }
        let pos = self.sessions.iter().position(|s| s.active);
        let next = pos.map_or(0, |p| (p + 1) % self.sessions.len());
        self.activate(self.sessions[next].index);
    }

    pub(super) fn goodbye(&mut self, done: oneshot::Sender<()>) {
        let paging_stopped = self.devices.stop_paging();
        let (ended, sessions_ended) = oneshot::channel();
        tokio::spawn(async move {
            paging_stopped.await;
            let _ = sessions_ended.await;
            let _ = done.send(());
        });
        if self.sessions.is_empty() {
            let _ = ended.send(());
            return;
        }
        println!("[core] goodbye to {} phone(s)", self.sessions.len());
        self.bye = Some(ended);
        for s in &self.sessions {
            self.tell(s.link, Do::Close);
        }
    }

    pub(super) fn close_by_link(&mut self, link: Link) {
        let Some(i) = self.sessions.iter().position(|s| s.link == link) else {
            return;
        };
        let closed = self.sessions.remove(i);
        self.devices
            .registry
            .clear_presence(&registry_ids(&closed.device));
        if self.sessions.is_empty() {
            self.next_index = 1;
            if let Some(done) = self.bye.take() {
                let _ = done.send(());
            }
            self.apply_when_idle();
        }
        if closed.active {
            match self.sessions.iter().position(|s| !s.active) {
                Some(next) => {
                    self.sessions[next].active = true;
                    println!(
                        "[core] session #{} ends, #{} moves up",
                        closed.index, self.sessions[next].index
                    );
                    self.on_active_changed(Some(next), true);
                }
                None => {
                    println!("[core] session #{} ends, nothing left", closed.index);
                    self.on_active_changed(None, true);
                }
            }
        }
        self.changed();
    }

    pub(super) fn close_on_transport(&mut self, ids: &Ids, transport: Transport) {
        if let Some(i) = self.by_identity(ids)
            && self.sessions[i].transport == transport
        {
            self.tell(self.sessions[i].link, Do::Close);
        }
    }

    pub(super) fn on_active_changed(&mut self, next: Option<usize>, had_prev: bool) {
        self.attention = None;
        self.activity(|a| *a = Activity::default());
        match next {
            Some(i) => {
                let (level, ramp) = (self.sessions[i].duck_level, self.sessions[i].duck_ramp_ms);
                self.duck(level, ramp);
                if !had_prev {
                    self.duck_level = 1.0;
                    self.duck_ramp_ms = RAMP_UP_MS;
                }
                self.tell(self.sessions[i].link, Do::Keyframe);
            }
            None => {
                self.plane.dispose();
                self.clusters.dispose();
                self.duck_level = 1.0;
                self.duck_ramp_ms = RAMP_UP_MS;
            }
        }
    }

    pub(super) fn changed(&mut self) {
        let owner = self.active().map(|s| s.link);
        if owner != self.video_owner {
            if let Some(old) = self.video_owner {
                self.tell(old, Do::VideoActive(false));
            }
            self.video_owner = owner;
            if let Some(new) = owner {
                self.tell(new, Do::VideoActive(true));
                self.push_all_levels();
            }
        }
        self.push_wired(false);
        self.publish();
    }

    pub(super) fn push_wired(&mut self, again: bool) {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| matches!(s.link, Link::Aa(_)) && s.transport == Transport::Usb)
            .flat_map(|s| {
                let d = &s.device;
                [&d.instance_id, &d.bt_mac, &d.usb_serial]
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect();
        if let Some(io) = self.hands_free.wired(ids, again) {
            let _ = self.phone_io.send(io);
        }
    }

    pub(super) fn session_of(&self, ids: &devices::Ids) -> Option<(u32, bool)> {
        let ids = session_ids(ids);
        if !ids.any() {
            return None;
        }
        let i = self.sessions.iter().position(|s| s.device.overlaps(&ids))?;
        Some((i as u32 + 1, self.sessions[i].active))
    }

    pub(super) fn targets(&self) -> Vec<(String, Option<String>)> {
        let on_cable = |mac: &str| {
            self.sessions.iter().any(|s| {
                s.transport == Transport::Usb
                    && s.device
                        .bt_mac
                        .as_deref()
                        .is_some_and(|m| m.eq_ignore_ascii_case(mac))
            })
        };
        devices::reconnect_targets(
            &self.devices.registry,
            devices::Paging::of(&self.hub.config()),
            &|ids| self.session_of(ids).is_some(),
            &on_cable,
        )
    }

    pub(super) fn select_device(&mut self, id: &str) {
        let ids = match self.devices.registry.by_id(id) {
            Some(e) => e.ids(),
            None => devices::Ids {
                bt_mac: Some(id.to_string()),
                wifi_mac: Some(id.to_string()),
                usb_udid: Some(id.to_string()),
                instance_id: Some(id.to_string()),
                ..Default::default()
            },
        };
        match self.by_identity(&session_ids(&ids)) {
            Some(i) => {
                println!("[core] picked {id}, session #{}", self.sessions[i].index);
                let index = self.sessions[i].index;
                self.activate(index);
            }
            None => {
                let entry = self.devices.registry.by_id(id);
                let protocol = entry.and_then(|e| e.view_protocol());
                let mac = entry
                    .and_then(|e| e.bt_mac().map(str::to_string))
                    .or_else(|| id.contains(':').then(|| id.to_string()));
                if let Some(mac) = mac {
                    tokio::spawn(devices::wake(mac, protocol));
                }
            }
        }
    }

    pub(super) fn forget_device(&mut self, id: &str) {
        let Some(gone) = self.devices.registry.forget(id) else {
            return;
        };
        if let Some(i) = self.by_identity(&session_ids(&gone.ids())) {
            let session = self.sessions[i].link;
            println!(
                "[core] forget {id} ends session #{}",
                self.sessions[i].index
            );
            let (cp, aa) = (self.cp.clone(), self.aa.clone());
            tokio::spawn(async move {
                tokio::time::sleep(FORGET_GRACE).await;
                match session {
                    Link::Cp(id) => cp.send(CpCmd::Close { id }),
                    Link::Aa(id) => aa.send(AaCmd::Close { id }),
                }
            });
        }
        if let Some(mac) = gone.bt_mac() {
            tokio::spawn(devices::unpair(mac.to_string()));
        }
        self.publish();
    }

    pub(super) fn publish(&mut self) {
        let list = devices::views(&self.devices.registry, &|ids| self.session_of(ids));
        let targets = self.targets();
        self.devices.page(targets, false);
        let main = self.front_main();
        let (cluster, aux) = (self.wants.cluster, self.wants.aux);
        let position = self.sessions.iter().position(|s| s.active);
        let sessions = Sessions {
            active: position.map(|i| self.sessions[i].link.protocol()),
            position: position.map_or(0, |i| i as u32 + 1),
            total: self.sessions.len() as u32,
        };
        let (now_playing, mut navigation) = self
            .active()
            .map(|s| (s.now_playing.clone(), s.navigation.clone()))
            .unwrap_or_default();
        written_out(
            &mut navigation,
            nav_text::Language::of(&self.config_seen.language),
        );
        self.hub.update(|s| {
            s.front.main = main;
            s.front.cluster = cluster;
            s.front.aux = aux;
            s.sessions = sessions;
            s.now_playing = now_playing;
            s.navigation = navigation;
            s.devices = list;
            s.cluster = self.cluster.clone();
        });
    }
}
