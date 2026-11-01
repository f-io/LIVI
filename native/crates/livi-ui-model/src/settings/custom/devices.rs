// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::message::Action;
use livi_core_proto::state::{DeviceStatus, DeviceView, Protocol};
use serde_json::Value;

use super::info;
use crate::i18n::Strings;
use crate::settings::view::{Device, Local, Presence, Row, RowKind, Tool};

pub fn devices(state: &Value, t: &Strings) -> Vec<Row> {
    let list: Vec<DeviceView> =
        serde_json::from_value(state["devices"].clone()).unwrap_or_default();
    if list.is_empty() {
        return vec![info(
            t.t("settings.noPairedDevices", "No paired devices"),
            None,
            String::new(),
        )];
    }
    list.iter().map(|d| device(d, t)).collect()
}

fn device(d: &DeviceView, t: &Strings) -> Row {
    let presence = match d.status {
        DeviceStatus::Active => Presence::Active,
        DeviceStatus::Available => Presence::Available,
        DeviceStatus::Offline => Presence::Offline,
    };
    let selectable = d.session.is_some() && presence != Presence::Offline;
    let label = [&d.name, &d.model]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| d.id.clone());
    Row {
        kind: RowKind::Device,
        label,
        enabled: selectable,
        dimmed: presence == Presence::Offline,
        action: selectable.then(|| Action::SelectDevice { id: d.id.clone() }),
        local: selectable.then_some(Local::ToProjection),
        device: Some(Device {
            protocol_icon: match d.protocol {
                Some(Protocol::Carplay) => "PhoneIphone",
                Some(Protocol::Androidauto) => "Android",
                None => "DirectionsCar",
            },
            transport_icon: match d.last_transport.as_deref() {
                Some("usb") => Some("CableOutlined"),
                Some("wifi") => Some("WifiOutlined"),
                _ => None,
            },
            battery: d.battery_level.map(|l| {
                (
                    l.round().clamp(0.0, 100.0) as u8,
                    d.battery_charging == Some(true),
                )
            }),
            signal: d.signal_strength.map(|s| s.round().clamp(0.0, 5.0) as u8),
            presence,
        }),
        tools: vec![Tool {
            icon: "Close",
            label: t.t("settings.forgetDevice", "Delete device"),
            on: false,
            tint: None,
            enabled: true,
            switch: false,
            action: Action::ForgetDevice { id: d.id.clone() },
        }],
        ..Row::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_devices_with_a_session_can_be_picked() {
        let s = json!({ "devices": [
            { "id": "a", "name": "iPhone", "protocol": "carplay", "lastTransport": "wifi",
              "status": "active", "batteryLevel": 54.4, "batteryCharging": true, "signalStrength": 3, "session": 1 },
            { "id": "b", "model": "Pixel 8", "protocol": "androidauto", "status": "offline", "session": 2 },
            { "id": "c", "status": "available" }
        ] });
        let rows = devices(&s, &Strings::new("en"));
        assert_eq!(rows[0].label, "iPhone");
        assert!(rows[0].enabled);
        let d = rows[0].device.as_ref().unwrap();
        assert_eq!(d.battery, Some((54, true)));
        assert_eq!(d.transport_icon, Some("WifiOutlined"));
        assert_eq!(rows[1].label, "Pixel 8");
        assert!(!rows[1].enabled && rows[1].dimmed);
        assert_eq!(rows[2].label, "c");
        assert!(rows[2].action.is_none());
    }

    #[test]
    fn no_devices_says_so() {
        let rows = devices(&json!({}), &Strings::new("en"));
        assert_eq!(rows[0].kind, RowKind::Info);
    }
}
