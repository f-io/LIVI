// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_transport_id: Option<String>,
    #[serde(flatten)]
    pub kind: HelperKind,
}

impl From<HelperKind> for HelperEvent {
    fn from(kind: HelperKind) -> Self {
        Self {
            cid: None,
            phone_id: None,
            usb_transport_id: None,
            kind,
        }
    }
}

impl HelperEvent {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceSource {
    Bt,
    Carkit,
    Bonjour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StationEvent {
    Joined,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallPhase {
    Ringing,
    Active,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum HelperKind {
    #[serde(rename = "device")]
    Device {
        src: DeviceSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bt_mac: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usb_udid: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ip: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "device-gone")]
    DeviceGone { src: DeviceSource, usb_udid: String },
    #[serde(rename = "wired-start")]
    WiredStart { usb_udid: String, ip: String },
    #[serde(rename = "wifi")]
    Wifi {
        event: StationEvent,
        mac: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ip: Option<String>,
    },
    #[serde(rename = "link")]
    Link { up: bool },
    #[serde(rename = "deviceTime")]
    DeviceTime { utc_offset_minutes: i32 },
    #[serde(rename = "power")]
    Power {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        charging: Option<bool>,
    },
    #[serde(rename = "cellular")]
    Cellular {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cellular_supported: Option<bool>,
    },
    #[serde(rename = "call")]
    Call {
        phase: CallPhase,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "nowplaying")]
    NowPlaying(Map<String, Value>),
    #[serde(rename = "navigation")]
    Navigation(Map<String, Value>),
    #[serde(rename = "albumart")]
    AlbumArt(Map<String, Value>),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(v: Value) -> HelperEvent {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn the_wire_names_stay_as_before() {
        let wired = HelperEvent {
            phone_id: Some("GG:HH".into()),
            ..HelperKind::WiredStart {
                usb_udid: "u3".into(),
                ip: "::ffff:127.0.0.1".into(),
            }
            .into()
        };
        assert_eq!(
            serde_json::to_value(&wired).unwrap(),
            json!({ "type": "wired-start", "usbUdid": "u3", "ip": "::ffff:127.0.0.1", "phoneId": "GG:HH" })
        );
        let seen = HelperEvent::from(HelperKind::Device {
            src: DeviceSource::Bonjour,
            bt_mac: Some("aa:bb".into()),
            usb_udid: None,
            ip: Some("10.0.0.2".into()),
            name: None,
        });
        assert_eq!(
            serde_json::to_value(&seen).unwrap(),
            json!({ "type": "device", "src": "bonjour", "btMac": "aa:bb", "ip": "10.0.0.2" })
        );
        assert_eq!(
            HelperEvent::from(HelperKind::Link { up: false }).to_line(),
            r#"{"type":"link","up":false}"#
        );
    }

    #[test]
    fn every_kind_reads_back() {
        for v in [
            json!({ "type": "device-gone", "src": "carkit", "usbUdid": "u1" }),
            json!({ "type": "wifi", "event": "left", "mac": "aa:01", "ip": "1.2.3.4" }),
            json!({ "type": "deviceTime", "utcOffsetMinutes": -120 }),
            json!({ "type": "power", "level": 80, "charging": true }),
            json!({ "type": "cellular", "signal": 3, "carrier": "X", "cellularSupported": true }),
            json!({ "type": "call", "phase": "ringing", "number": "123" }),
            json!({ "type": "nowplaying", "title": "Song", "cid": "c1" }),
            json!({ "type": "navigation", "status": 1, "maneuvers": [] }),
        ] {
            let ev = read(v.clone());
            assert_eq!(serde_json::to_value(&ev).unwrap(), v);
        }
        let np = read(json!({ "type": "nowplaying", "title": "Song", "cid": "c1" }));
        assert_eq!(np.cid.as_deref(), Some("c1"));
        assert!(
            matches!(np.kind, HelperKind::NowPlaying(m) if m.get("title") == Some(&json!("Song")) && !m.contains_key("cid"))
        );
    }

    #[test]
    fn an_unknown_or_broken_event_is_refused() {
        for v in [
            json!({ "type": "dance" }),
            json!({ "type": "wifi" }),
            json!({ "type": "call", "phase": "dialing" }),
            json!({ "up": true }),
        ] {
            assert!(
                serde_json::from_value::<HelperEvent>(v.clone()).is_err(),
                "{v}"
            );
        }
    }
}
