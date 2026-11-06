// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Deserialize, Serialize};

use crate::state::{LaneGuidance, Maneuver, Navigation, NowPlaying};

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
    NowPlaying(NowPlayingPatch),
    #[serde(rename = "navigation")]
    Navigation(Box<NavigationPatch>),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NowPlayingPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
}

impl NowPlayingPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn apply_to(self, np: &mut NowPlaying) {
        take(&mut np.title, self.title);
        take(&mut np.artist, self.artist);
        take(&mut np.album, self.album);
        take(&mut np.app, self.app);
        take(&mut np.duration_ms, self.duration_ms);
        take(&mut np.elapsed_ms, self.elapsed_ms);
        take(&mut np.playing, self.playing);
        take(&mut np.artwork, self.artwork.filter(|a| !a.is_empty()));
    }
}

fn take<T>(into: &mut Option<T>, from: Option<T>) {
    if from.is_some() {
        *into = from;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Turn {
    pub maneuver_type: Option<u32>,
    pub turn_side: Option<u32>,
    pub junction_type: Option<u32>,
    pub turn_angle: Option<i32>,
    pub after_road_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NavigationPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub road_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_epoch: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_destination: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_to_destination: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_to_destination_display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_to_destination_units: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_distance_display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_distance_units: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maneuver_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_in_app: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_guidance_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_guidance_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_guidance_shown: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_supports_route_guidance: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_time_zone_offset_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maneuvers: Option<Vec<Maneuver>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_maneuvers: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_guidance: Option<Vec<LaneGuidance>>,
}

impl NavigationPatch {
    pub fn apply_to(self, nav: &mut Navigation) {
        if let Some(status) = self.status {
            if status == 0 {
                *nav = Navigation::default();
            }
            nav.active = Some(status != 0);
        }
        take(&mut nav.order_type, self.order_type);
        take(&mut nav.road_name, self.road_name);
        take(&mut nav.destination_name, self.destination_name);
        take(&mut nav.eta, self.eta_epoch.filter(|v| *v > 0.0));
        take(&mut nav.time_to_destination, self.time_to_destination);
        take(
            &mut nav.distance_to_destination,
            self.distance_to_destination,
        );
        take(
            &mut nav.distance_to_destination_display,
            self.distance_to_destination_display,
        );
        take(
            &mut nav.distance_to_destination_units,
            self.distance_to_destination_units,
        );
        take(&mut nav.remain_distance, self.remain_distance);
        take(
            &mut nav.remain_distance_display,
            self.remain_distance_display,
        );
        take(&mut nav.remain_distance_units, self.remain_distance_units);
        take(&mut nav.maneuver_count, self.maneuver_count);
        take(&mut nav.visible_in_app, self.visible_in_app);
        take(&mut nav.lane_guidance_index, self.lane_guidance_index);
        take(&mut nav.lane_guidance_count, self.lane_guidance_count);
        take(&mut nav.lane_guidance_shown, self.lane_guidance_shown);
        take(&mut nav.app_name, self.app_name);
        take(
            &mut nav.source_supports_route_guidance,
            self.source_supports_route_guidance,
        );
        take(
            &mut nav.destination_time_zone_offset_minutes,
            self.destination_time_zone_offset_minutes,
        );
        if let Some(turn) = self.turn {
            nav.maneuver_type = turn.maneuver_type;
            nav.turn_side = turn.turn_side;
            nav.junction_type = turn.junction_type;
            nav.turn_angle = turn.turn_angle;
            nav.after_road_name = turn.after_road_name;
        }
        if let Some(v) = self.maneuvers {
            nav.maneuvers = v;
        }
        if let Some(v) = self.current_maneuvers {
            nav.current_maneuvers = v;
        }
        if let Some(v) = self.lane_guidance {
            nav.lane_guidance = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

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
            json!({ "type": "nowplaying", "artwork": "AAEC" }),
            json!({ "type": "navigation", "status": 1, "turn": { "maneuverType": 4, "turnSide": null, "junctionType": null, "turnAngle": null, "afterRoadName": "A9" } }),
        ] {
            let ev = read(v.clone());
            assert_eq!(serde_json::to_value(&ev).unwrap(), v);
        }
        let np = read(json!({ "type": "nowplaying", "title": "Song", "cid": "c1" }));
        assert_eq!(np.cid.as_deref(), Some("c1"));
        assert!(matches!(np.kind, HelperKind::NowPlaying(p) if p.title.as_deref() == Some("Song")));
    }

    #[test]
    fn a_now_playing_patch_overwrites_only_what_it_carries() {
        let mut np = NowPlaying {
            title: Some("Old".into()),
            artist: Some("Band".into()),
            artwork: Some("AAEC".into()),
            ..Default::default()
        };
        NowPlayingPatch {
            title: Some("New".into()),
            playing: Some(true),
            artwork: Some(String::new()),
            ..Default::default()
        }
        .apply_to(&mut np);
        assert_eq!(np.title.as_deref(), Some("New"));
        assert_eq!(np.artist.as_deref(), Some("Band"));
        assert_eq!(np.playing, Some(true));
        assert_eq!(np.artwork.as_deref(), Some("AAEC"));
    }

    #[test]
    fn a_navigation_patch_merges_and_a_stop_clears() {
        let mut nav = Navigation::default();
        NavigationPatch {
            status: Some(1),
            road_name: Some("A9".into()),
            eta_epoch: Some(0.0),
            turn: Some(Turn {
                maneuver_type: Some(4),
                ..Default::default()
            }),
            ..Default::default()
        }
        .apply_to(&mut nav);
        assert_eq!(nav.active, Some(true));
        assert_eq!(nav.road_name.as_deref(), Some("A9"));
        assert_eq!(nav.eta, None);
        assert_eq!(nav.maneuver_type, Some(4));
        NavigationPatch {
            turn: Some(Turn::default()),
            ..Default::default()
        }
        .apply_to(&mut nav);
        assert_eq!(nav.maneuver_type, None);
        assert_eq!(nav.road_name.as_deref(), Some("A9"));
        NavigationPatch {
            status: Some(0),
            ..Default::default()
        }
        .apply_to(&mut nav);
        assert_eq!(
            nav,
            Navigation {
                active: Some(false),
                ..Default::default()
            }
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
