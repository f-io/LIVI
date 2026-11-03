// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeSet;

use livi_core_proto::state::{Front, VideoPlayback};
use livi_ui_model::shell::{Look, look};
use serde_json::Value;

use crate::PhoneState;
use crate::dashboard_ui::{DashReader, DashView};
use crate::media::{DabView, MediaReader, MediaView};
use livi_ui_model::dashboard::screens::secondary;
use livi_ui_model::mask::{Mask, Stream, mask};
use livi_ui_model::overlay;

pub enum Update {
    Media(MediaView),
    Dab(DabView),
    Dash(Box<DashView>),
    DashOn(livi_core_proto::state::Screen, Box<DashView>),
    Secondary(Vec<livi_ui_model::dashboard::screens::Secondary>),
    Front(Front),
    Phone(PhoneState),
    Look(Look),
    Kiosk(bool),
    Settings,
    Spectrum(Vec<f32>),
    Open(String),
    Volume(f64),
    PhoneName(String),
    Camera(bool),
    MainMask(Option<Mask>),
    Fonts(Vec<String>),
    Video(Option<VideoPlayback>),
}

#[derive(Default)]
pub struct Follow {
    media: MediaReader,
    front: Option<Front>,
    phone: Option<PhoneState>,
    look: Option<Look>,
    kiosk: Option<bool>,
    dash: DashReader,
    others: Vec<(livi_core_proto::state::Screen, DashReader)>,
    secondary: Option<Vec<livi_ui_model::dashboard::screens::Secondary>>,
    volume: overlay::Volume,
    phones: overlay::Phone,
    started: bool,
    main_mask: Option<Option<Mask>>,
    fonts: Option<Vec<String>>,
    video: Option<Option<VideoPlayback>>,
}

impl Follow {
    pub fn changed(&mut self, keys: Option<&BTreeSet<String>>, state: &Value) -> Vec<Update> {
        let all = keys.is_none_or(|k| k.contains(""));
        let hit = |key: &str| all || keys.is_some_and(|k| k.contains(key));
        let mut out = Vec::new();
        if hit("nowPlaying") || hit("sessions") {
            out.push(Update::Media(self.media.read(state)));
        }
        if hit("dab")
            && let Some(d) = self.media.read_dab(state)
        {
            out.push(Update::Dab(d));
        }
        if hit("sessions") {
            let phone = if state["sessions"]["active"].is_null() {
                PhoneState::None
            } else {
                PhoneState::Streaming
            };
            if self.phone != Some(phone) {
                self.phone = Some(phone);
                out.push(Update::Phone(phone));
            }
        }
        if hit("front") {
            let front: Option<Front> = serde_json::from_value(state["front"]["main"].clone()).ok();
            if let Some(front) = front
                && self.front != Some(front)
            {
                self.front = Some(front);
                out.push(Update::Front(front));
            }
        }
        if hit("config") {
            let next = look(&state["config"]);
            if self.look.as_ref() != Some(&next) {
                self.look = Some(next.clone());
                out.push(Update::Look(next));
            }
            let kiosk = state["config"]["kiosk"]["main"].as_bool().unwrap_or(false);
            if self.kiosk != Some(kiosk) {
                self.kiosk = Some(kiosk);
                out.push(Update::Kiosk(kiosk));
            }
        }
        if hit("videoPlayback") {
            let next = serde_json::from_value(state["videoPlayback"].clone())
                .ok()
                .flatten();
            if self.video != Some(next) {
                self.video = Some(next);
                out.push(Update::Video(next));
            }
        }
        if hit("sessions") || hit("config") {
            let next = mask(state, Stream::Main);
            if self.main_mask != Some(next) {
                self.main_mask = Some(next);
                out.push(Update::MainMask(next));
            }
        }
        if hit("telemetry")
            || hit("config")
            || hit("cluster")
            || hit("navigation")
            || hit("sessions")
        {
            let dark = self.look.as_ref().is_none_or(|l| l.dark);
            out.push(Update::Dash(Box::new(self.dash.read(
                state,
                livi_core_proto::state::Screen::Main,
                dark,
            ))));
            let wanted = secondary(state);
            if self.secondary.as_ref() != Some(&wanted) {
                self.secondary = Some(wanted.clone());
                out.push(Update::Secondary(wanted.clone()));
            }
            self.others
                .retain(|(screen, _)| wanted.iter().any(|w| w.screen == *screen));
            for w in wanted.iter().filter(|w| w.dashboard) {
                let reader = match self.others.iter_mut().position(|(s, _)| *s == w.screen) {
                    Some(i) => &mut self.others[i].1,
                    None => {
                        self.others.push((w.screen, DashReader::default()));
                        &mut self.others.last_mut().expect("just pushed").1
                    }
                };
                out.push(Update::DashOn(
                    w.screen,
                    Box::new(reader.read(state, w.screen, dark)),
                ));
            }
        }
        if hit("system") {
            let fonts: Vec<String> = state["system"]["fonts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect();
            if self.fonts.as_ref() != Some(&fonts) {
                self.fonts = Some(fonts.clone());
                out.push(Update::Fonts(fonts));
            }
        }
        if hit("cameras") {
            let found = state["cameras"].as_array().is_some_and(|c| !c.is_empty());
            out.push(Update::Camera(found));
        }
        if livi_ui_model::settings::STATE_READ
            .iter()
            .any(|key| hit(key))
        {
            out.push(Update::Settings);
        }
        if hit("config")
            && let Some(level) = self.volume.update(state)
        {
            out.push(Update::Volume(level));
        }
        if (hit("config") || hit("devices"))
            && let Some(name) = self.phones.update(state)
        {
            out.push(Update::PhoneName(name));
        }
        if keys.is_none()
            && !std::mem::replace(&mut self.started, true)
            && std::env::var_os("LIVI_UI_PAGE").is_none()
        {
            let start = state["system"]["startPage"].as_str().unwrap_or("/");
            out.push(Update::Open(start.to_string()));
        }
        out
    }
}
