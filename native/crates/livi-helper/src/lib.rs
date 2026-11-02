// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod aa;
pub mod embedded;
pub mod link;
pub mod mfi;
pub mod wired;

use livi_core_proto::helper::HelperStart;
use livi_runtime::bringup::CpConfig;
use livi_runtime::ident::{Identity, Theme, Transport};
use livi_runtime::vehicle::{Fuels, Plugs};
use livi_wifi::{Band, Channel};

pub const DEFAULT_NAME: &str = "LIVI";
pub const DEFAULT_PASSPHRASE: &str = "12345678";
pub const DEFAULT_CHANNEL: u32 = 36;
pub const SOURCE_VERSION: &str = "950.7.1";
pub const AVAILABLE_CURRENT_MA: u16 = 500;

fn or(value: &str, default: &str) -> String {
    if value.is_empty() {
        default.to_string()
    } else {
        value.to_string()
    }
}

pub fn carplay(start: &HelperStart, transport: Transport) -> (Identity, CpConfig) {
    let s = &start.settings;
    let name = or(&s.car_name, DEFAULT_NAME);
    let theme = Theme::parse(&s.theme);
    let channel = if s.channel == 0 {
        DEFAULT_CHANNEL
    } else {
        s.channel
    };
    let cp = CpConfig {
        wifi_iface: s.wifi_iface.clone(),
        ssid: name.clone(),
        passphrase: or(&s.passphrase, DEFAULT_PASSPHRASE),
        channel: Channel::of_setting(
            Band::of_setting(&s.wifi_type).unwrap_or(Band::Ghz5),
            channel,
        ),
        airplay_port: start.airplay_port.map_or(0, u32::from),
        source_version: SOURCE_VERSION.to_string(),
        public_key: start.pairing_id.clone(),
        transport,
        av_iface: None,
        av_address: None,
        av_iface_late: None,
        available_current_ma: AVAILABLE_CURRENT_MA,
        device_id: Some(start.device_id.clone()).filter(|id| !id.is_empty()),
        ap_mac: None,
        ap_on_air: None,
        on_cable: None,
        start_again: None,
        theme: theme.clone(),
    };
    let identity = Identity {
        name: name.clone(),
        ssid: name,
        bt_mac: [0; 6],
        fuels: Fuels::parse(&s.fuels),
        plugs: Plugs::parse(&s.connectors),
        theme,
    };
    (identity, cp)
}

pub(crate) fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}
