// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn front_main(&self) -> Front {
        if self.reverse_camera() {
            return Front::Camera;
        }
        let wants = if self.attention.is_some() {
            Front::Projection
        } else {
            self.wants.main
        };
        match wants {
            Front::Projection if self.active().is_some_and(|s| s.video) => Front::Projection,
            Front::Projection => Front::Livi,
            other => other,
        }
    }

    pub(super) fn asked(&mut self, asked: PerScreen<Front>) {
        let before = self.wants.main;
        let last = std::mem::replace(&mut self.asked, asked);
        let camera = self.reverse_camera();
        for screen in [Screen::Main, Screen::Cluster, Screen::Aux] {
            if asked.get(screen) != last.get(screen) && !(camera && screen == Screen::Main) {
                *self.wants.get_mut(screen) = *asked.get(screen);
            }
        }
        if !camera && asked.main != last.main && asked.main != Front::Projection {
            self.attention = None;
        }
        let cluster_on = shows_cluster(&self.wants);
        if cluster_on != self.cluster_on {
            self.cluster_on = cluster_on;
            self.cp.send(CpCmd::ClusterActive(cluster_on));
            self.aa.send(AaCmd::ClusterActive(cluster_on));
        }
        if self.wants.main == Front::Projection && before != Front::Projection {
            if self.attention.is_none() {
                self.to_active(Do::Home);
            }
            self.keyframe();
        }
        self.publish();
    }

    pub(super) fn reverse_camera(&self) -> bool {
        let cfg = &self.config_seen;
        self.reversing
            && self.camera_found
            && cfg.auto_switch_on_reverse
            && cfg.camera.main
            && !cfg.camera_id.is_empty()
    }

    pub(super) fn camera_found(&mut self, found: bool) {
        let before = self.reverse_camera();
        self.camera_found = found;
        self.camera_follows(before);
    }

    pub(super) fn camera_follows(&mut self, before: bool) {
        let now = self.reverse_camera();
        if now != before {
            println!(
                "[core] reversing camera {}",
                if now { "in front" } else { "gone" }
            );
            self.publish();
        }
    }

    pub(super) fn attend(&mut self, kind: Attention, active: bool) {
        if active {
            if self.front_main() == Front::Projection {
                if let Some((armed, _)) = self.attention.as_mut() {
                    *armed = kind;
                }
                return;
            }
            self.attention = Some((kind, self.wants.main));
        } else {
            match self.attention {
                Some((armed, back)) if armed == kind => {
                    self.attention = None;
                    self.wants.main = back;
                }
                _ => return,
            }
        }
        self.publish();
    }

    pub(super) fn host_ui(&mut self) {
        self.attention = None;
        if self.wants.main == Front::Projection {
            self.wants.main = Front::Livi;
        }
        self.publish();
    }

    pub(super) fn keyframe(&self) {
        self.to_active(Do::Keyframe);
    }
}
