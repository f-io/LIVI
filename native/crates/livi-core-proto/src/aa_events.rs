// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AaTransport {
    Wifi,
    Usb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    FastForward,
    Rewind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum AaHelperEvent {
    Hfp {
        up: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mac: Option<String>,
    },
    PhoneBattery {
        mac: String,
        pct: u8,
    },
    Sco {
        up: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mtu: Option<usize>,
    },
    Input {
        command: RemoteCommand,
    },
    AaDevice {
        bt_mac: String,
        instance_id: String,
        usb_serial: String,
    },
    AaSession {
        socket: String,
        peer: String,
        transport: AaTransport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        serial: Option<String>,
    },
}

impl AaHelperEvent {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn every_event_reads_back_with_its_wire_names() {
        for wire in [
            json!({ "event": "hfp", "up": true, "mac": "AA:00" }),
            json!({ "event": "hfp", "up": false }),
            json!({ "event": "phone-battery", "mac": "AA:00", "pct": 80 }),
            json!({ "event": "sco", "up": true, "mtu": 60 }),
            json!({ "event": "sco", "up": false }),
            json!({ "event": "input", "command": "next" }),
            json!({ "event": "input", "command": "playPause" }),
            json!({ "event": "input", "command": "fastForward" }),
            json!({ "event": "aa-device", "btMac": "AA:00", "instanceId": "i1", "usbSerial": "" }),
            json!({ "event": "aa-session", "socket": "/tmp/s", "peer": "10.0.0.3", "transport": "wifi" }),
            json!({ "event": "aa-session", "socket": "/tmp/s", "peer": "usb", "transport": "usb", "serial": "R5" }),
        ] {
            let event: AaHelperEvent = serde_json::from_value(wire.clone()).unwrap();
            let back: Value = serde_json::from_str(&event.to_line()).unwrap();
            assert_eq!(back, wire);
        }
    }

    #[test]
    fn an_unknown_or_broken_event_is_refused() {
        for wire in [
            json!({ "event": "dance" }),
            json!({ "event": "input", "command": "eject" }),
            json!({ "event": "aa-session" }),
            json!({ "event": "aa-session", "socket": "/tmp/s", "peer": "x", "transport": "bt" }),
            json!({ "event": "phone-battery", "mac": "AA:00", "pct": 300 }),
            json!({ "type": "nowplaying" }),
        ] {
            assert!(
                serde_json::from_value::<AaHelperEvent>(wire.clone()).is_err(),
                "{wire}"
            );
        }
    }
}
