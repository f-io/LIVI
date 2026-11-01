// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod bindings;
mod car;
mod dab;
mod devices;
pub mod files;
mod gps;
pub mod icon;
mod mfi;
mod system;
pub mod touch;

use livi_core_proto::message::Action;
use serde_json::Value;

use crate::i18n::Strings;
use crate::icons::settings_icon;
use crate::settings::node::CustomPage;
use crate::settings::view::{Dialog, Facts, Row, RowKind};

pub use bindings::{binding_row, key_label, set_binding};
pub use devices::devices;

pub(crate) const FAVORITE_TINT: [u8; 3] = [0xff, 0xa7, 0x26];
pub(crate) const DASH: &str = "—";

#[derive(Debug, Clone, PartialEq)]
pub enum Watch {
    LinkSpeed,
    Act(Action),
}

pub fn rows(page: CustomPage, state: &Value, t: &Strings, facts: &Facts) -> Option<Vec<Row>> {
    Some(match page {
        CustomPage::DabReceiver => dab::receiver(&dab::dab(state), t),
        CustomPage::DabStations => dab::stations(&dab::dab(state), t),
        CustomPage::About => system::about(t, facts),
        CustomPage::SoftwareUpdate => system::update(state, t, facts),
        CustomPage::Restart | CustomPage::PowerOff => Vec::new(),
        CustomPage::WifiLinkInfo => system::link_speed(state, t),
        CustomPage::CarReadings => car::readings(state, t),
        CustomPage::DashboardLayouts => car::layouts(state, t),
        CustomPage::GpsInfo => gps::data(state, t),
        CustomPage::GpsHwInfo => gps::hardware(state, t),
        CustomPage::IconUploader => icon::rows(state, t),
        CustomPage::MfiCertificate => mfi::rows(state, t),
        CustomPage::Touch(screen) => touch::rows(screen, state, t),
        _ => return None,
    })
}

pub fn dialog(page: CustomPage, state: &Value, t: &Strings, facts: &Facts) -> Option<Dialog> {
    match page {
        CustomPage::SoftwareUpdate => system::update_dialog(state, t, facts),
        _ => None,
    }
}

pub fn watch(page: CustomPage) -> Option<Watch> {
    match page {
        CustomPage::WifiLinkInfo => Some(Watch::LinkSpeed),
        CustomPage::SoftwareUpdate => Some(Watch::Act(Action::CheckUpdate)),
        CustomPage::Restart => Some(Watch::Act(Action::Restart)),
        CustomPage::PowerOff => Some(Watch::Act(Action::Quit)),
        _ => None,
    }
}

pub(crate) fn info(label: String, icon: Option<&'static str>, value: String) -> Row {
    Row {
        kind: RowKind::Info,
        label,
        icon: icon.and_then(settings_icon),
        value,
        enabled: true,
        ..Row::default()
    }
}

pub(crate) fn mono(label: String, value: String) -> Row {
    Row {
        mono: true,
        ..info(label, None, value)
    }
}
