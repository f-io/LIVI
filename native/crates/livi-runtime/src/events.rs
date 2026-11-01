// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use iap2_csm::CsmMessage;
use iap2_csm::messages::communications::CallStateUpdate;
use iap2_csm::messages::communications::CommunicationsUpdate;
use iap2_csm::messages::device_notifications::DeviceTimeUpdate;
use iap2_csm::messages::now_playing::{NowPlayingUpdate, PlaybackStatus};
use iap2_csm::messages::power::PowerUpdate;
use iap2_csm::messages::route_guidance::{
    LaneGuidanceInformation, RouteGuidanceManeuverUpdate, RouteGuidanceUpdate,
};
use serde_json::{Map, Value, json};

use crate::framing::frame_msg_id;

pub fn device_json(frame: &[u8], usb_udid: Option<&str>) -> Option<String> {
    use iap2_csm::messages::car_play::DeviceTransportIdentifierNotification;

    if frame_msg_id(frame)? != 0x4E0E {
        return None;
    }
    let m = DeviceTransportIdentifierNotification::decode(frame).ok()?;
    let mut o = Obj::new("device");
    o.str("src", if usb_udid.is_some() { "carkit" } else { "bt" });
    if !m.bluetooth_transport_id.is_empty() {
        o.str("btMac", &m.bluetooth_transport_id);
    }
    if let Some(udid) = usb_udid {
        o.str("usbUdid", udid);
    }
    o.finish()
}

pub fn wired_start_json(usb_udid: &str, ip: &str) -> String {
    let mut o = Obj::new("wired-start");
    o.str("usbUdid", usb_udid);
    o.str("ip", ip);
    o.finish().unwrap_or_default()
}

#[derive(Debug, Default, Clone)]
pub struct EventTag {
    pub phone_id: Option<String>,
    pub cid: Option<String>,
    pub usb_transport_id: Option<String>,
    pub navigation: Navigation,
}

const ROUTE_CAPACITY: usize = 50;

#[derive(Debug, Default, Clone)]
pub struct Navigation {
    maneuvers: BTreeMap<u16, RouteGuidanceManeuverUpdate>,
    lanes: BTreeMap<u16, LaneGuidanceInformation>,
    current: Vec<u16>,
    count: Option<u16>,
    lane_index: Option<u16>,
}

impl Navigation {
    fn prune(&mut self) {
        if let Some(first) = self.current.iter().min() {
            self.maneuvers.retain(|i, _| i >= first);
        }
        if let Some(count) = self.count {
            self.maneuvers.retain(|i, _| *i < count);
        }
        while self.maneuvers.len() > ROUTE_CAPACITY {
            self.maneuvers.pop_first();
        }
        if let Some(first) = self.lane_index {
            self.lanes.retain(|i, _| *i >= first);
        }
        while self.lanes.len() > ROUTE_CAPACITY {
            self.lanes.pop_first();
        }
    }

    fn lists(&self, o: &mut Obj) {
        let maneuvers: Vec<Value> = self.maneuvers.values().map(maneuver_value).collect();
        let lanes: Vec<Value> = self.lanes.values().map(lane_guidance_value).collect();
        o.raw("maneuvers", &Value::from(maneuvers).to_string());
        o.raw("currentManeuvers", &json!(self.current).to_string());
        o.raw("laneGuidance", &Value::from(lanes).to_string());
    }

    fn current_turn(&self) -> Option<&RouteGuidanceManeuverUpdate> {
        self.current.first().and_then(|i| self.maneuvers.get(i))
    }
}

impl EventTag {
    pub fn learn(&mut self, frame: &[u8]) {
        use iap2_csm::messages::car_play::DeviceTransportIdentifierNotification;

        if frame_msg_id(frame) != Some(0x4E0E) {
            return;
        }
        if let Ok(m) = DeviceTransportIdentifierNotification::decode(frame) {
            if !m.bluetooth_transport_id.is_empty() {
                self.phone_id = Some(m.bluetooth_transport_id);
            }
            if !m.usb_transport_id.is_empty() {
                self.usb_transport_id = Some(m.usb_transport_id);
            }
        }
    }

    pub fn navigation_json(&mut self, frame: &[u8]) -> Option<String> {
        match frame_msg_id(frame)? {
            0x5201 => {
                let m = RouteGuidanceUpdate::decode(frame).ok()?;
                let nav = &mut self.navigation;
                if m.state == Some(0) {
                    *nav = Navigation::default();
                }
                if let Some(list) = m.current_maneuver_list.as_deref() {
                    nav.current = indices(list);
                }
                nav.count = m.maneuver_count.or(nav.count);
                nav.lane_index = m.lane_guidance_index.or(nav.lane_index);
                nav.prune();
                let mut o = Obj::new("navigation");
                if let Some(v) = m.state {
                    o.num("status", v);
                }
                if let Some(v) = m.maneuver_state {
                    o.num("orderType", v);
                }
                if let Some(v) = &m.current_road_name {
                    o.str("roadName", v);
                }
                if let Some(v) = &m.destination_name {
                    o.str("destinationName", v);
                }
                if let Some(v) = m.eta {
                    o.num("etaEpoch", v);
                }
                if let Some(v) = m.time_remaining {
                    o.num("timeToDestination", v);
                }
                if let Some(v) = m.distance_remaining {
                    o.num("distanceToDestination", v);
                }
                if let Some(v) = &m.distance_remaining_display {
                    o.str("distanceToDestinationDisplay", v);
                }
                if let Some(v) = m.distance_remaining_units {
                    o.num("distanceToDestinationUnits", v);
                }
                if let Some(v) = m.distance_to_maneuver {
                    o.num("remainDistance", v);
                }
                if let Some(v) = &m.distance_to_maneuver_display {
                    o.str("remainDistanceDisplay", v);
                }
                if let Some(v) = m.distance_to_maneuver_units {
                    o.num("remainDistanceUnits", v);
                }
                if let Some(v) = m.maneuver_count {
                    o.num("maneuverCount", v);
                }
                if let Some(v) = m.visible_in_app {
                    o.bool("visibleInApp", v);
                }
                if let Some(v) = m.lane_guidance_index {
                    o.num("laneGuidanceIndex", v);
                }
                if let Some(v) = m.lane_guidance_count {
                    o.num("laneGuidanceCount", v);
                }
                if let Some(v) = m.lane_guidance_shown {
                    o.bool("laneGuidanceShown", v);
                }
                if let Some(v) = &m.source_name {
                    o.str("appName", v);
                }
                if let Some(v) = m.source_supports_route_guidance {
                    o.bool("sourceSupportsRouteGuidance", v);
                }
                if let Some(v) = m.destination_time_zone_offset_minutes {
                    o.num("destinationTimeZoneOffsetMinutes", v);
                }
                if let Some(turn) = nav.current_turn() {
                    maneuver_fields(&mut o, turn);
                }
                nav.lists(&mut o);
                o.finish()
            }
            0x5202 => {
                let m = RouteGuidanceManeuverUpdate::decode(frame).ok()?;
                let index = m.index?;
                let nav = &mut self.navigation;
                nav.maneuvers.insert(index, m);
                nav.prune();
                let mut o = Obj::new("navigation");
                if nav.current.first() == Some(&index)
                    && let Some(turn) = nav.current_turn()
                {
                    maneuver_fields(&mut o, turn);
                }
                nav.lists(&mut o);
                o.finish()
            }
            0x5204 => {
                let m = LaneGuidanceInformation::decode(frame).ok()?;
                let index = m.index?;
                let nav = &mut self.navigation;
                nav.lanes.insert(index, m);
                nav.prune();
                let mut o = Obj::new("navigation");
                nav.lists(&mut o);
                o.finish()
            }
            _ => None,
        }
    }

    pub fn apply(&self, json: String) -> String {
        let mut extra = String::new();
        for (key, value) in [
            ("cid", self.cid.as_deref()),
            ("phoneId", self.phone_id.as_deref()),
            ("usbTransportId", self.usb_transport_id.as_deref()),
        ] {
            if let Some(v) = value {
                extra.push_str(&format!(",\"{key}\":\"{}\"", escape(v)));
            }
        }
        if extra.is_empty() || !json.ends_with('}') {
            return json;
        }
        format!("{}{extra}}}", &json[..json.len() - 1])
    }
}

pub fn device_time(frame: &[u8]) -> Option<i64> {
    if frame_msg_id(frame)? != 0x4E0B {
        return None;
    }
    DeviceTimeUpdate::decode(frame)
        .ok()?
        .seconds_since_reference_date
}

pub fn to_json(frame: &[u8]) -> Option<String> {
    match frame_msg_id(frame)? {
        0x5001 => now_playing(frame),
        0xAE01 => power(frame),
        0x4158 => cellular(frame),
        0x4155 => call(frame),
        0x4E0B => utc_offset(frame),
        _ => None,
    }
}

struct Obj {
    parts: Vec<String>,
}

impl Obj {
    fn new(kind: &str) -> Self {
        Self {
            parts: vec![format!("\"type\":\"{kind}\"")],
        }
    }
    fn raw(&mut self, key: &str, json: &str) {
        self.parts.push(format!("\"{key}\":{json}"));
    }

    fn str(&mut self, key: &str, value: &str) {
        self.parts.push(format!("\"{key}\":\"{}\"", escape(value)));
    }
    fn num(&mut self, key: &str, value: impl std::fmt::Display) {
        self.parts.push(format!("\"{key}\":{value}"));
    }
    fn bool(&mut self, key: &str, value: bool) {
        self.parts.push(format!("\"{key}\":{value}"));
    }
    fn finish(self) -> Option<String> {
        if self.parts.len() <= 1 {
            None
        } else {
            Some(format!("{{{}}}", self.parts.join(",")))
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn now_playing(frame: &[u8]) -> Option<String> {
    let m = NowPlayingUpdate::decode(frame).ok()?;
    let mut o = Obj::new("nowplaying");
    if let Some(mi) = m.media_item_attributes {
        if let Some(t) = mi.title {
            o.str("title", &t);
        }
        if let Some(a) = mi.artist {
            o.str("artist", &a);
        }
        if let Some(a) = mi.album {
            o.str("album", &a);
        }
        if let Some(d) = mi.duration_ms {
            o.num("durationMs", d);
        }
    }
    if let Some(pb) = m.playback_attributes {
        if let Some(s) = pb.status {
            o.num("playing", if s == PlaybackStatus::Playing { 1 } else { 0 });
        }
        if let Some(e) = pb.elapsed_ms {
            o.num("elapsedMs", e);
        }
        if let Some(n) = pb.app_name {
            o.str("appName", &n);
        }
    }
    o.finish()
}

fn power(frame: &[u8]) -> Option<String> {
    let m = PowerUpdate::decode(frame).ok()?;
    let mut o = Obj::new("power");
    if let Some(l) = m.battery_charge_level {
        o.num("level", l);
    }
    if let Some(c) = m.is_external_charger_connected {
        o.bool("charging", c);
    }
    o.finish()
}

fn cellular(frame: &[u8]) -> Option<String> {
    let m = CommunicationsUpdate::decode(frame).ok()?;
    let mut o = Obj::new("cellular");
    if let Some(s) = m.signal_strength.as_ref().and_then(|v| v.first()) {
        o.num("signal", s);
    }
    if let Some(c) = m.carrier_name {
        o.str("carrier", &c);
    }
    if let Some(s) = m.cellular_supported {
        o.bool("cellularSupported", s);
    }
    o.finish()
}

fn utc_offset(frame: &[u8]) -> Option<String> {
    let m = DeviceTimeUpdate::decode(frame).ok()?;
    let minutes = i32::from(m.time_zone_offset_minutes?);
    let mut o = Obj::new("deviceTime");
    o.num("utcOffsetMinutes", minutes);
    o.finish()
}

fn call(frame: &[u8]) -> Option<String> {
    let m = CallStateUpdate::decode(frame).ok()?;
    let status = m.status?;
    let phase = match status {
        0 => "ended",
        2 => "ringing",
        _ => "active",
    };
    let mut o = Obj::new("call");
    o.str("phase", phase);
    if phase != "ended" {
        if let Some(n) = m.remote_id {
            o.str("number", &n);
        }
        if let Some(n) = m.display_name {
            o.str("name", &n);
        }
    }
    o.finish()
}

fn indices(list: &[u8]) -> Vec<u16> {
    list.as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_be_bytes(*b))
        .collect()
}

fn maneuver_fields(o: &mut Obj, turn: &RouteGuidanceManeuverUpdate) {
    o.raw("maneuverType", &opt(turn.maneuver_type));
    o.raw("turnSide", &opt(turn.driving_side));
    o.raw("junctionType", &opt(turn.junction_type));
    o.raw("turnAngle", &opt(turn.exit_angle));
    o.raw(
        "afterRoadName",
        &opt(turn.after_maneuver_road_name.as_deref()),
    );
}

fn opt<T: Into<Value>>(v: Option<T>) -> String {
    v.map_or(Value::Null, Into::into).to_string()
}

fn set(map: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(v) = value {
        map.insert(key.into(), v.into());
    }
}

fn maneuver_value(m: &RouteGuidanceManeuverUpdate) -> Value {
    let mut o = Map::new();
    set(&mut o, "index", m.index);
    set(&mut o, "maneuverType", m.maneuver_type);
    set(&mut o, "turnSide", m.driving_side);
    set(&mut o, "junctionType", m.junction_type);
    set(&mut o, "turnAngle", m.exit_angle);
    set(&mut o, "afterRoadName", m.after_maneuver_road_name.clone());
    set(&mut o, "description", m.description.clone());
    set(&mut o, "distanceBetween", m.distance_between);
    set(
        &mut o,
        "distanceBetweenDisplay",
        m.distance_between_display.clone(),
    );
    set(&mut o, "distanceBetweenUnits", m.distance_between_units);
    o.insert("junctionAngles".into(), json!(m.junction_element_angles));
    set(&mut o, "linkedLaneGuidance", m.linked_lane_guidance);
    set(&mut o, "exitInfo", m.exit_info.clone());
    Value::Object(o)
}

fn lane_guidance_value(g: &LaneGuidanceInformation) -> Value {
    let lanes: Vec<Value> = g
        .lanes
        .iter()
        .map(|l| {
            let mut o = Map::new();
            set(&mut o, "index", l.index);
            set(&mut o, "status", l.status);
            o.insert("angles".into(), json!(l.angles));
            set(&mut o, "highlightAngle", l.highlight_angle);
            Value::Object(o)
        })
        .collect();
    let mut o = Map::new();
    set(&mut o, "index", g.index);
    set(&mut o, "description", g.description.clone());
    o.insert("lanes".into(), Value::from(lanes));
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(index: u16, kind: u8, road: &str) -> RouteGuidanceManeuverUpdate {
        RouteGuidanceManeuverUpdate {
            display_component_id: Some(0),
            index: Some(index),
            description: None,
            maneuver_type: Some(kind),
            after_maneuver_road_name: Some(road.into()),
            distance_between: None,
            distance_between_display: None,
            distance_between_units: None,
            driving_side: None,
            junction_type: None,
            junction_element_angles: Vec::new(),
            exit_angle: None,
            linked_lane_guidance: None,
            exit_info: None,
        }
    }

    fn guidance(current: &[u16], state: u8) -> RouteGuidanceUpdate {
        RouteGuidanceUpdate {
            display_component_id: None,
            state: Some(state),
            maneuver_state: None,
            current_road_name: None,
            destination_name: None,
            eta: None,
            time_remaining: None,
            distance_remaining: None,
            distance_remaining_display: None,
            distance_remaining_units: None,
            distance_to_maneuver: None,
            distance_to_maneuver_display: None,
            distance_to_maneuver_units: None,
            current_maneuver_list: Some(current.iter().flat_map(|i| i.to_be_bytes()).collect()),
            maneuver_count: None,
            visible_in_app: None,
            lane_guidance_index: None,
            lane_guidance_count: None,
            lane_guidance_shown: None,
            source_name: None,
            source_supports_route_guidance: None,
            destination_time_zone_offset_minutes: None,
        }
    }

    fn parsed(json: Option<String>) -> serde_json::Value {
        serde_json::from_str(&json.expect("an event")).unwrap()
    }

    #[test]
    fn the_current_turn_rides_along_with_the_guidance() {
        let mut tag = EventTag::default();
        let mut exit = turn(2, 4, "Elbchaussee");
        exit.exit_angle = Some(-90);
        tag.navigation_json(&exit.encode());

        let mut update = guidance(&[2], 1);
        update.maneuver_state = Some(2);
        update.current_road_name = Some("Reeperbahn".into());
        update.distance_remaining = Some(12000);
        update.distance_to_maneuver = Some(350);
        update.distance_to_maneuver_display = Some("350".into());
        update.distance_to_maneuver_units = Some(2);
        update.source_name = Some("Maps".into());
        let v = parsed(tag.navigation_json(&update.encode()));
        assert_eq!(v["status"], 1);
        assert_eq!(v["orderType"], 2);
        assert_eq!(v["roadName"], "Reeperbahn");
        assert_eq!(v["remainDistance"], 350);
        assert_eq!(v["remainDistanceDisplay"], "350");
        assert_eq!(v["remainDistanceUnits"], 2);
        assert_eq!(v["appName"], "Maps");
        assert_eq!(v["maneuverType"], 4);
        assert_eq!(v["turnAngle"], -90);
        assert_eq!(v["afterRoadName"], "Elbchaussee");

        let mut replaced = turn(2, 4, "Elbchaussee");
        replaced.maneuver_type = None;
        replaced.junction_type = Some(1);
        let v = parsed(tag.navigation_json(&replaced.encode()));
        assert_eq!(v["junctionType"], 1);
        assert!(
            v["maneuverType"].is_null(),
            "an update replaces the turn: {v}"
        );
        assert!(v["turnAngle"].is_null(), "an update replaces the turn: {v}");
    }

    #[test]
    fn the_whole_route_is_kept_and_passed_turns_drop_off() {
        let mut tag = EventTag::default();
        for (index, kind, road) in [(0, 1, "A"), (1, 2, "B"), (2, 12, "C")] {
            let mut t = turn(index, kind, road);
            t.distance_between = Some(100 * u32::from(index));
            t.junction_element_angles = vec![-90, 45];
            let v = parsed(tag.navigation_json(&t.encode()));
            assert_eq!(
                v["maneuvers"].as_array().unwrap().len(),
                usize::from(index) + 1
            );
        }

        let v = parsed(tag.navigation_json(&guidance(&[0], 1).encode()));
        let list = v["maneuvers"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[1]["afterRoadName"], "B");
        assert_eq!(list[2]["distanceBetween"], 200);
        assert_eq!(list[0]["junctionAngles"], serde_json::json!([-90, 45]));
        assert_eq!(v["currentManeuvers"], serde_json::json!([0]));

        let v = parsed(tag.navigation_json(&guidance(&[1, 2], 1).encode()));
        let list = v["maneuvers"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["index"], 1);
        assert_eq!(v["currentManeuvers"], serde_json::json!([1, 2]));

        let mut ended = guidance(&[], 0);
        ended.current_maneuver_list = None;
        let v = parsed(tag.navigation_json(&ended.encode()));
        assert_eq!(v["maneuvers"], serde_json::json!([]));
        assert!(tag.navigation.maneuvers.is_empty());
    }

    #[test]
    fn lane_guidance_is_kept_from_the_current_entry_on() {
        use iap2_csm::messages::route_guidance::LaneInformation;

        let mut tag = EventTag::default();
        for index in 0..3u16 {
            let lanes = LaneGuidanceInformation {
                display_component_id: Some(0),
                index: Some(index),
                lanes: vec![
                    LaneInformation {
                        index: Some(0),
                        status: Some(0),
                        angles: vec![0],
                        highlight_angle: None,
                    },
                    LaneInformation {
                        index: Some(1),
                        status: Some(2),
                        angles: vec![0, 45],
                        highlight_angle: Some(45),
                    },
                ],
                description: None,
            };
            tag.navigation_json(&lanes.encode());
        }
        let mut update = guidance(&[0], 1);
        update.lane_guidance_index = Some(1);
        update.lane_guidance_shown = Some(true);
        let v = parsed(tag.navigation_json(&update.encode()));
        assert_eq!(v["laneGuidanceShown"], true);
        let lanes = v["laneGuidance"].as_array().unwrap();
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0]["index"], 1);
        assert_eq!(lanes[0]["lanes"][1]["status"], 2);
        assert_eq!(lanes[0]["lanes"][1]["highlightAngle"], 45);
    }

    use iap2_csm::messages::now_playing::{MediaItemAttributes, PlaybackAttributes};

    #[test]
    fn now_playing_json() {
        let frame = NowPlayingUpdate {
            media_item_attributes: Some(MediaItemAttributes {
                persistent_id: None,
                title: Some("Song \"A\"".into()),
                duration_ms: Some(180000),
                album: None,
                artist: Some("Artist".into()),
                album_artist: None,
                genre: None,
                artwork_ftid: None,
            }),
            playback_attributes: Some(PlaybackAttributes {
                status: Some(PlaybackStatus::Playing),
                elapsed_ms: Some(42000),
                app_name: Some("Music".into()),
                app_bundle_id: None,
            }),
        }
        .encode();
        let json = to_json(&frame).unwrap();
        assert!(json.contains("\"type\":\"nowplaying\""));
        assert!(json.contains("\"title\":\"Song \\\"A\\\"\""));
        assert!(json.contains("\"playing\":1"));
        assert!(json.contains("\"appName\":\"Music\""));
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;
    use iap2_csm::messages::car_play::DeviceTransportIdentifierNotification;

    fn frame() -> Vec<u8> {
        DeviceTransportIdentifierNotification {
            bluetooth_transport_id: "0C:6A:C4:4E:F3:2A".into(),
            usb_transport_id: "usb-1".into(),
        }
        .encode()
    }

    #[test]
    fn the_wired_start_names_the_cable_the_phone_and_the_address() {
        let mut tag = EventTag::default();
        tag.learn(&frame());
        let json = tag.apply(wired_start_json(
            "00008120000924CE2E51A01E",
            "fe80::80a:a1ca",
        ));
        assert_eq!(
            json,
            "{\"type\":\"wired-start\",\"usbUdid\":\"00008120000924CE2E51A01E\",\"ip\":\"fe80::80a:a1ca\",\"phoneId\":\"0C:6A:C4:4E:F3:2A\",\"usbTransportId\":\"usb-1\"}"
        );
    }

    #[test]
    fn wired_marks_usb_udid() {
        let json = device_json(&frame(), Some("00008120000924CE2E51A01E")).unwrap();
        assert!(json.contains("\"src\":\"carkit\""));
        assert!(json.contains("\"usbUdid\":\"00008120000924CE2E51A01E\""));
        assert!(json.contains("\"btMac\":\"0C:6A:C4:4E:F3:2A\""));
    }

    #[test]
    fn metadata_carries_identity_for_routing() {
        let mut tag = EventTag {
            cid: Some("ctrl-1".into()),
            ..Default::default()
        };
        tag.learn(&frame());
        let np = NowPlayingUpdate {
            media_item_attributes: Some(iap2_csm::messages::now_playing::MediaItemAttributes {
                persistent_id: None,
                title: Some("Song".into()),
                duration_ms: None,
                album: None,
                artist: None,
                album_artist: None,
                genre: None,
                artwork_ftid: None,
            }),
            playback_attributes: None,
        }
        .encode();
        let json = tag.apply(to_json(&np).unwrap());
        assert!(json.contains("\"phoneId\":\"0C:6A:C4:4E:F3:2A\""), "{json}");
        assert!(json.contains("\"cid\":\"ctrl-1\""), "{json}");
        assert!(json.contains("\"usbTransportId\":\"usb-1\""), "{json}");
        assert!(json.starts_with("{\"type\":\"nowplaying\""));
    }

    #[test]
    fn daylight_saving_is_not_counted_twice() {
        let summer = DeviceTimeUpdate {
            seconds_since_reference_date: Some(0),
            time_zone_offset_minutes: Some(120),
            daylight_savings_offset_minutes: Some(60),
        };
        assert_eq!(
            to_json(&summer.encode()).as_deref(),
            Some("{\"type\":\"deviceTime\",\"utcOffsetMinutes\":120}")
        );
        let winter = DeviceTimeUpdate {
            time_zone_offset_minutes: Some(60),
            daylight_savings_offset_minutes: None,
            ..summer
        };
        assert!(to_json(&winter.encode()).unwrap().ends_with(":60}"));
    }

    #[test]
    fn a_time_without_an_offset_names_no_zone() {
        let update = DeviceTimeUpdate {
            seconds_since_reference_date: Some(0),
            time_zone_offset_minutes: None,
            daylight_savings_offset_minutes: None,
        };
        assert_eq!(to_json(&update.encode()), None);
    }

    #[test]
    fn wireless_has_no_udid() {
        let json = device_json(&frame(), None).unwrap();
        assert!(json.contains("\"src\":\"bt\""));
        assert!(!json.contains("usbUdid"));
    }
}
