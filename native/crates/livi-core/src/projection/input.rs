// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn media(&self, control: MediaControl) {
        self.to_active(Do::Media(control));
    }

    pub(super) fn seek(&self, ms: u32) {
        if let Some(s) = self.active().filter(|s| matches!(s.link, Link::Cp(_))) {
            self.devices.seek(ms, s.device.bt_mac.clone());
        }
    }

    pub(super) fn input(&mut self, input: Input) {
        match input {
            Input::Pointer {
                screen: Screen::Main,
                points,
            } => self.pointer(&points),
            Input::Pointer { .. } => {}
            Input::Key { code, down } => self.key(&code, down),
        }
    }

    pub(super) fn pointer(&self, points: &[Point]) {
        if points.is_empty() || self.front_main() != Front::Projection {
            return;
        }
        match self.active().map(|s| s.link) {
            Some(Link::Cp(id)) => {
                let touches = points
                    .iter()
                    .map(|p| Touch {
                        x: p.x.clamp(0.0, 1.0),
                        y: p.y.clamp(0.0, 1.0),
                        down: !matches!(p.phase, Phase::Up | Phase::Cancel),
                    })
                    .collect();
                self.cp.stack(id, StackCmd::Touches(touches));
            }
            Some(Link::Aa(id)) => {
                let crop = *self.aa_crop.borrow();
                let touches = points
                    .iter()
                    .map(|p| {
                        let (x, y) = android_auto::to_stream(
                            p.x.clamp(0.0, 1.0),
                            p.y.clamp(0.0, 1.0),
                            crop.as_ref(),
                        );
                        let action = match p.phase {
                            Phase::Down => TouchAction::Down,
                            Phase::Move => TouchAction::Move,
                            Phase::Up | Phase::Cancel => TouchAction::Up,
                        };
                        TouchItem {
                            id: p.id,
                            x,
                            y,
                            action,
                        }
                    })
                    .collect();
                self.aa.session(id, SessionCmd::MultiTouch(touches));
            }
            None => {}
        }
    }

    pub(super) fn dab_media_ask(&self, control: MediaControl) -> Option<crate::dab::DabAsk> {
        let state = self.hub.watch();
        let s = state.borrow();
        crate::dab::media_ask(&s.dab, s.sessions.active.is_some(), control)
    }

    pub(super) fn key(&mut self, code: &str, down: bool) {
        let Some(key) = bound(&self.bindings, code) else {
            return;
        };
        if key == Key::ClusterLayout {
            if down && self.cluster.step(1) {
                self.cluster_changed();
            }
            return;
        }
        if let (Some(control), Some(dab)) = (key.media_control(), &self.dab)
            && let Some(ask) = self.dab_media_ask(control)
        {
            if down {
                let _ = dab.send(ask);
            }
            return;
        }
        if self.front_main() != Front::Projection && !key.works_behind() {
            return;
        }
        match self.active().map(|s| s.link) {
            Some(Link::Cp(id)) if down => self.cp_key(id, key),
            Some(Link::Aa(id)) => self.aa_key(id, key, down),
            _ => {}
        }
    }

    pub(super) fn open_app(&self, target: &str) {
        let Some(link) = self.active().map(|e| e.link) else {
            return;
        };
        match link {
            Link::Cp(id) => {
                let url = if target.contains(':') {
                    target.to_string()
                } else {
                    format!("{target}:")
                };
                self.cp.stack(id, StackCmd::RequestUi(url));
            }
            Link::Aa(id) => {
                if let Some(cmd) = aa_open(target) {
                    self.aa.session(id, SessionCmd::Command(cmd));
                }
            }
        }
    }

    pub(super) fn cp_key(&self, id: SessionId, key: Key) {
        let none = KnobState::default();
        let cmd = match key {
            Key::Left => knob(KnobState {
                x: -KNOB_DEFLECT,
                ..none
            }),
            Key::Right => knob(KnobState {
                x: KNOB_DEFLECT,
                ..none
            }),
            Key::Up => knob(KnobState {
                y: -KNOB_DEFLECT,
                ..none
            }),
            Key::Down => knob(KnobState {
                y: KNOB_DEFLECT,
                ..none
            }),
            Key::SelectDown | Key::KnobDown => StackCmd::KnobSelect(true),
            Key::SelectUp | Key::KnobUp => StackCmd::KnobSelect(false),
            Key::Back => knob(KnobState { back: true, ..none }),
            Key::Home => knob(KnobState { home: true, ..none }),
            Key::KnobLeft => knob(KnobState {
                wheel: -1.0,
                ..none
            }),
            Key::KnobRight => knob(KnobState { wheel: 1.0, ..none }),
            Key::PlayPause => StackCmd::Media(media_button::PLAY_PAUSE),
            Key::Play => StackCmd::Media(media_button::PLAY),
            Key::Pause => StackCmd::Media(media_button::PAUSE),
            Key::Next => StackCmd::Media(media_button::NEXT),
            Key::Prev => StackCmd::Media(media_button::PREV),
            Key::AcceptPhone => StackCmd::Telephony(telephony_button::HOOK_SWITCH),
            Key::RejectPhone => StackCmd::Telephony(telephony_button::DROP),
            Key::Phone(button) => StackCmd::Telephony(button),
            Key::VoiceAssistant => StackCmd::Siri,
            Key::ClusterContent => StackCmd::NextClusterContent,
            Key::CycleSession | Key::VoiceAssistantRelease | Key::ClusterLayout => return,
        };
        self.cp.stack(id, cmd);
        if key == Key::SelectDown {
            let cp = self.cp.clone();
            tokio::spawn(async move {
                tokio::time::sleep(SELECT_RELEASE).await;
                cp.stack(id, StackCmd::KnobSelect(false));
            });
        }
    }

    pub(super) fn aa_key(&self, id: AaId, key: Key, down: bool) {
        use telephony_button as t;
        let command = match (key, down) {
            (Key::VoiceAssistant, false) => Command::VoiceAssistantRelease,
            (_, false) => return,
            (Key::Left, _) => Command::Left,
            (Key::Right, _) => Command::Right,
            (Key::Up, _) => Command::Up,
            (Key::Down, _) => Command::Down,
            (Key::SelectDown, _) => Command::SelectDown,
            (Key::SelectUp, _) => Command::SelectUp,
            (Key::KnobDown, _) => Command::KnobDown,
            (Key::KnobUp, _) => Command::KnobUp,
            (Key::Back, _) => Command::Back,
            (Key::Home, _) => Command::Home,
            (Key::KnobLeft, _) => Command::KnobLeft,
            (Key::KnobRight, _) => Command::KnobRight,
            (Key::PlayPause, _) => Command::PlayPause,
            (Key::Play, _) => Command::Play,
            (Key::Pause, _) => Command::Pause,
            (Key::Next, _) => Command::Next,
            (Key::Prev, _) => Command::Prev,
            (Key::AcceptPhone, _) => Command::AcceptPhone,
            (Key::RejectPhone, _) => Command::RejectPhone,
            (Key::VoiceAssistant, _) => Command::VoiceAssistant,
            (Key::VoiceAssistantRelease, _) => Command::VoiceAssistantRelease,
            (Key::Phone(b), _) if b == t::STAR => Command::PhoneKeyStar,
            (Key::Phone(b), _) if b == t::POUND => Command::PhoneKeyHash,
            (Key::Phone(b), _) if b == t::HOOK_SWITCH => Command::PhoneKeyHookSwitch,
            (Key::Phone(b), _) => match b.wrapping_sub(t::KEY0) {
                0 => Command::PhoneKey0,
                1 => Command::PhoneKey1,
                2 => Command::PhoneKey2,
                3 => Command::PhoneKey3,
                4 => Command::PhoneKey4,
                5 => Command::PhoneKey5,
                6 => Command::PhoneKey6,
                7 => Command::PhoneKey7,
                8 => Command::PhoneKey8,
                9 => Command::PhoneKey9,
                _ => return,
            },
            (Key::CycleSession | Key::ClusterLayout | Key::ClusterContent, _) => return,
        };
        self.aa.session(id, SessionCmd::Command(command));
        if key == Key::SelectDown {
            let aa = self.aa.clone();
            tokio::spawn(async move {
                tokio::time::sleep(SELECT_RELEASE).await;
                aa.session(id, SessionCmd::Command(Command::SelectUp));
            });
        }
    }
}
