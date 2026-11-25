// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::{Map, Value};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    Now,
    Apply,
    Restart,
}

const APPLY: &[&str] = &[
    "displayMode",
    "displayConnector",
    "displayRotation",
    "screenWidthMm",
    "screenHeightMm",
];

const RESTART: &[&str] = &[
    "wirelessAaEnabled",
    "wirelessCpEnabled",
    "airplayEnabled",
    "carPlayTheme",
    "carPlayVehicleModel",
    "carPlayDockPosition",
    "carPlayVehicleClimate",
    "carPlaySourceVersion",
    "carPlayMfiPowerGpio",
    "carPlayMfiFiles",
    "carplayIcon120",
    "carplayIcon180",
    "carplayIcon256",
    "projectionWidth",
    "projectionHeight",
    "projectionFps",
    "projectionDpi",
    "projectionViewAreaTop",
    "projectionViewAreaBottom",
    "projectionViewAreaLeft",
    "projectionViewAreaRight",
    "projectionSafeAreaTop",
    "projectionSafeAreaBottom",
    "projectionSafeAreaLeft",
    "projectionSafeAreaRight",
    "projectionSafeAreaDrawOutside",
    "clusterWidth",
    "clusterHeight",
    "clusterFps",
    "clusterDpi",
    "clusterViewAreaTop",
    "clusterViewAreaBottom",
    "clusterViewAreaLeft",
    "clusterViewAreaRight",
    "clusterSafeAreaTop",
    "clusterSafeAreaBottom",
    "clusterSafeAreaLeft",
    "clusterSafeAreaRight",
    "wifiType",
    "wifiChannel",
    "wifiChannelWidth",
    "country",
    "wifiInterface",
    "btAdapter",
    "wifiDedicatedInterface",
    "disableAudioOutput",
    "autoConn",
    "carName",
    "oemName",
    "wifiPassword",
    "samplingFrequency",
    "hand",
    "carType",
    "evConnectorTypes",
    "debugLogging",
];

pub fn takes(key: &str) -> Takes {
    if APPLY.contains(&key) {
        Takes::Apply
    } else if RESTART.contains(&key) {
        Takes::Restart
    } else {
        Takes::Now
    }
}

fn waits(key: &str, sessions_open: bool) -> bool {
    match takes(key) {
        Takes::Now => false,
        Takes::Apply => true,
        Takes::Restart => sessions_open,
    }
}

fn fields(config: &Config) -> Map<String, Value> {
    match serde_json::to_value(config) {
        Ok(Value::Object(fields)) => fields,
        _ => Map::new(),
    }
}

pub fn held(config: &Config, applied: &Config, sessions_open: bool) -> Config {
    let applied = fields(applied);
    let mut next = fields(config);
    for (key, value) in &mut next {
        if waits(key, sessions_open)
            && let Some(old) = applied.get(key)
        {
            *value = old.clone();
        }
    }
    serde_json::from_value(Value::Object(next)).unwrap_or_else(|_| config.clone())
}

pub fn saved(config: &Config, applied: &Config) -> Config {
    held(config, applied, true)
}

pub fn restarts_sessions(before: &Config, after: &Config) -> bool {
    let after = fields(after);
    fields(before)
        .iter()
        .any(|(key, old)| takes(key) == Takes::Restart && after.get(key) != Some(old))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defaults;

    #[test]
    fn every_listed_setting_exists_once() {
        let known = fields(&defaults());
        for key in APPLY.iter().chain(RESTART) {
            assert!(known.contains_key(*key), "{key} is no setting");
        }
        assert!(APPLY.iter().all(|k| !RESTART.contains(k)));
    }

    #[test]
    fn a_setting_waits_by_its_kind() {
        let applied = defaults();
        let mut edited = applied.clone();
        edited.dark_mode = !applied.dark_mode;
        edited.screen_width_mm.main = 344;
        edited.wireless_aa_enabled = !applied.wireless_aa_enabled;

        let idle = held(&edited, &applied, false);
        assert_eq!(idle.dark_mode, edited.dark_mode);
        assert_eq!(idle.wireless_aa_enabled, edited.wireless_aa_enabled);
        assert_eq!(idle.screen_width_mm.main, applied.screen_width_mm.main);

        let busy = held(&edited, &applied, true);
        assert_eq!(busy.dark_mode, edited.dark_mode);
        assert_eq!(busy.wireless_aa_enabled, applied.wireless_aa_enabled);
        assert_eq!(busy.screen_width_mm.main, applied.screen_width_mm.main);

        assert_eq!(saved(&edited, &applied), busy);
        assert_eq!(held(&applied, &applied, true), applied);
    }

    #[test]
    fn only_a_restart_setting_restarts_the_sessions() {
        let before = defaults();
        let mut screen = before.clone();
        screen.display_rotation.main = 90;
        assert!(!restarts_sessions(&before, &screen));
        let mut phone = before.clone();
        phone.car_name = "Volvo".into();
        assert!(restarts_sessions(&before, &phone));
    }
}
