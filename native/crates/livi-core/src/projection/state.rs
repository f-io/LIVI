// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn apply_settings(&mut self) {
        println!("[core] applying the settings: helper restart, phones reconnect");
        self.apply_due = None;
        self.hub.apply();
        self.helper_restart.notify_one();
        self.cp.send(CpCmd::DropSessions);
    }

    pub(super) fn apply_when_idle(&mut self) {
        self.apply_due = (self.sessions.is_empty() && self.hub.apply_pending())
            .then(|| tokio::time::Instant::now() + APPLY_AFTER_QUIET);
    }

    pub(super) fn apply_idle(&mut self) {
        self.apply_due = None;
        if !self.sessions.is_empty() {
            return;
        }
        if let Some((before, after)) = self.hub.apply()
            && helper::starts_differently(&before, &after)
        {
            println!("[core] settings applied with no phone connected, the helper restarts");
            self.helper_restart.notify_one();
        }
    }

    pub(super) fn activity(&self, change: impl FnOnce(&mut Activity)) {
        if let Some(activity) = &self.activity {
            activity.send_modify(change);
        }
    }

    pub(super) fn follow_phone(&mut self, choice: PhoneChoice) -> bool {
        match choice {
            PhoneChoice::Layout(layout) => self.cluster.follow(&layout, None),
            PhoneChoice::Palette { layout, palette } => {
                self.cluster.follow(&layout, Some(&palette))
            }
            PhoneChoice::Appearance(preference) => {
                let appearance = match preference {
                    Appearance::Automatic => AppearancePreference::Automatic,
                    Appearance::Light => AppearancePreference::Light,
                    Appearance::Dark => AppearancePreference::Dark,
                };
                let changed = self.cluster.appearance != appearance;
                self.cluster.appearance = appearance;
                changed
            }
        }
    }

    pub(super) fn on_state(&mut self, cfg: Config, telemetry: Map<String, Value>) {
        let camera = self.reverse_camera();
        if cfg != self.config_seen {
            self.on_config(&cfg);
            let language = cfg.language != self.config_seen.language;
            let theme = cfg.car_play_theme != self.config_seen.car_play_theme;
            if theme {
                self.follow_theme(&cfg);
            }
            let arranged = self.cluster.arrange(&cfg.dashboard_layouts);
            self.config_seen = cfg;
            if language || theme || arranged {
                self.publish();
            }
            self.apply_when_idle();
        }
        if telemetry != self.telemetry_seen {
            self.cp_telemetry
                .follow(&self.telemetry_seen, &telemetry, &self.cp);
            self.aa_telemetry
                .follow(&self.telemetry_seen, &telemetry, &self.aa);
            if let Some(reversing) = reversing(&telemetry) {
                self.reversing = reversing;
            }
            self.telemetry_seen = telemetry;
        }
        self.camera_follows(camera);
    }

    pub(super) fn on_config(&mut self, cfg: &Config) {
        let next = Levels::from(cfg);
        for kind in [
            AudioKind::Media,
            AudioKind::Alert,
            AudioKind::Speech,
            AudioKind::Call,
        ] {
            let (prev, to) = (self.levels.get(kind), next.get(kind).clamp(0.0, 1.0));
            if (prev - to).abs() < 0.0001 {
                continue;
            }
            match kind {
                AudioKind::Media => self.levels.music = to,
                AudioKind::Alert => self.levels.nav = to,
                AudioKind::Speech => self.levels.voice = to,
                AudioKind::Call => self.levels.call = to,
            }
            self.push_level(kind, if prev > to { RAMP_DOWN_MS } else { RAMP_UP_MS });
        }
        self.cp
            .send(CpCmd::InitialNightMode(cfg.appearance_mode.night_mode()));
        self.bindings = cfg.bindings.clone();
        self.stream_volumes_to_phone();
    }
}
