// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

const NEEDS_RESTART: &[&str] = &[
    "wirelessAaEnabled",
    "wirelessCpEnabled",
    "carPlayHlsVideo",
    "carPlayTheme",
    "carPlayVehicleModel",
    "carPlayDockPosition",
    "carPlayVehicleClimate",
    "displayMode",
    "displayConnector",
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
    "carplayIcon120",
    "carplayIcon180",
    "carplayIcon256",
    "carPlayMfiFiles",
];

pub fn sessions_open(state: &Value) -> bool {
    state["sessions"]["total"].as_u64().unwrap_or(0) > 0
}

pub fn pending(config: &Value, baseline: &Value) -> bool {
    NEEDS_RESTART.iter().any(|k| config[*k] != baseline[*k])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_restart_settings_count() {
        let base = json!({ "carName": "LIVI", "darkMode": true });
        assert!(!pending(
            &json!({ "carName": "LIVI", "darkMode": false }),
            &base
        ));
        assert!(pending(
            &json!({ "carName": "Volvo", "darkMode": true }),
            &base
        ));
    }
}
