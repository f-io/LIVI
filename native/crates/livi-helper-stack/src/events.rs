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
use livi_core_proto::helper_events::{
    CallPhase, DeviceSource, HelperEvent, HelperKind, NavigationPatch, NowPlayingPatch, Turn,
};
use livi_core_proto::state::{Lane, LaneGuidance, Maneuver};

use crate::framing::frame_msg_id;

pub fn device_event(frame: &[u8], usb_udid: Option<&str>) -> Option<HelperKind> {
    use iap2_csm::messages::car_play::DeviceTransportIdentifierNotification;

    if frame_msg_id(frame)? != 0x4E0E {
        return None;
    }
    let m = DeviceTransportIdentifierNotification::decode(frame).ok()?;
    let seen = HelperKind::Device {
        src: if usb_udid.is_some() {
            DeviceSource::Carkit
        } else {
            DeviceSource::Bt
        },
        bt_mac: Some(m.bluetooth_transport_id).filter(|m| !m.is_empty()),
        usb_udid: usb_udid.map(str::to_string),
        ip: None,
        name: None,
    };
    Some(seen)
}

pub fn wired_start_event(usb_udid: &str, ip: &str) -> HelperKind {
    HelperKind::WiredStart {
        usb_udid: usb_udid.to_string(),
        ip: ip.to_string(),
    }
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

    fn lists(&self, patch: &mut NavigationPatch) {
        patch.maneuvers = Some(self.maneuvers.values().map(maneuver).collect());
        patch.current_maneuvers = Some(self.current.iter().map(|&i| u32::from(i)).collect());
        patch.lane_guidance = Some(self.lanes.values().map(lane_guidance).collect());
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

    pub fn navigation_event(&mut self, frame: &[u8]) -> Option<HelperKind> {
        let mut patch = NavigationPatch::default();
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
                patch = NavigationPatch {
                    status: m.state,
                    order_type: m.maneuver_state.map(u32::from),
                    road_name: m.current_road_name,
                    destination_name: m.destination_name,
                    eta_epoch: m.eta.map(|v| v as f64),
                    time_to_destination: m.time_remaining.map(|v| v as f64),
                    distance_to_destination: m.distance_remaining.map(f64::from),
                    distance_to_destination_display: m.distance_remaining_display,
                    distance_to_destination_units: m.distance_remaining_units.map(u32::from),
                    remain_distance: m.distance_to_maneuver.map(f64::from),
                    remain_distance_display: m.distance_to_maneuver_display,
                    remain_distance_units: m.distance_to_maneuver_units.map(u32::from),
                    maneuver_count: m.maneuver_count.map(u32::from),
                    visible_in_app: m.visible_in_app,
                    lane_guidance_index: m.lane_guidance_index.map(u32::from),
                    lane_guidance_count: m.lane_guidance_count.map(u32::from),
                    lane_guidance_shown: m.lane_guidance_shown,
                    app_name: m.source_name,
                    source_supports_route_guidance: m.source_supports_route_guidance,
                    destination_time_zone_offset_minutes: m
                        .destination_time_zone_offset_minutes
                        .map(i32::from),
                    turn: nav.current_turn().map(turn),
                    ..Default::default()
                };
            }
            0x5202 => {
                let m = RouteGuidanceManeuverUpdate::decode(frame).ok()?;
                let index = m.index?;
                let nav = &mut self.navigation;
                nav.maneuvers.insert(index, m);
                nav.prune();
                if nav.current.first() == Some(&index) {
                    patch.turn = nav.current_turn().map(turn);
                }
            }
            0x5204 => {
                let m = LaneGuidanceInformation::decode(frame).ok()?;
                let index = m.index?;
                let nav = &mut self.navigation;
                nav.lanes.insert(index, m);
                nav.prune();
            }
            _ => return None,
        }
        self.navigation.lists(&mut patch);
        Some(HelperKind::Navigation(Box::new(patch)))
    }

    pub fn tagged(&self, kind: HelperKind) -> String {
        HelperEvent {
            cid: self.cid.clone(),
            phone_id: self.phone_id.clone(),
            usb_transport_id: self.usb_transport_id.clone(),
            kind,
        }
        .to_line()
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

pub fn phone_event(frame: &[u8]) -> Option<HelperKind> {
    match frame_msg_id(frame)? {
        0x5001 => now_playing(frame),
        0xAE01 => power(frame),
        0x4158 => cellular(frame),
        0x4155 => call(frame),
        0x4E0B => utc_offset(frame),
        _ => None,
    }
}

fn now_playing(frame: &[u8]) -> Option<HelperKind> {
    let m = NowPlayingUpdate::decode(frame).ok()?;
    let mut patch = NowPlayingPatch::default();
    if let Some(mi) = m.media_item_attributes {
        patch.title = mi.title;
        patch.artist = mi.artist;
        patch.album = mi.album;
        patch.duration_ms = mi.duration_ms.map(|v| v as f64);
    }
    if let Some(pb) = m.playback_attributes {
        patch.playing = pb.status.map(|s| s == PlaybackStatus::Playing);
        patch.elapsed_ms = pb.elapsed_ms.map(|v| v as f64);
        patch.app = pb.app_name;
    }
    (!patch.is_empty()).then_some(HelperKind::NowPlaying(patch))
}

fn power(frame: &[u8]) -> Option<HelperKind> {
    let m = PowerUpdate::decode(frame).ok()?;
    let (level, charging) = (m.battery_charge_level, m.is_external_charger_connected);
    (level.is_some() || charging.is_some()).then_some(HelperKind::Power { level, charging })
}

fn cellular(frame: &[u8]) -> Option<HelperKind> {
    let m = CommunicationsUpdate::decode(frame).ok()?;
    let signal = m.signal_strength.as_ref().and_then(|v| v.first()).copied();
    let (carrier, cellular_supported) = (m.carrier_name, m.cellular_supported);
    (signal.is_some() || carrier.is_some() || cellular_supported.is_some()).then_some(
        HelperKind::Cellular {
            signal,
            carrier,
            cellular_supported,
        },
    )
}

fn utc_offset(frame: &[u8]) -> Option<HelperKind> {
    let m = DeviceTimeUpdate::decode(frame).ok()?;
    let utc_offset_minutes = i32::from(m.time_zone_offset_minutes?);
    Some(HelperKind::DeviceTime { utc_offset_minutes })
}

fn call(frame: &[u8]) -> Option<HelperKind> {
    let m = CallStateUpdate::decode(frame).ok()?;
    let phase = match m.status? {
        0 => CallPhase::Ended,
        2 => CallPhase::Ringing,
        _ => CallPhase::Active,
    };
    let ongoing = phase != CallPhase::Ended;
    Some(HelperKind::Call {
        phase,
        number: m.remote_id.filter(|_| ongoing),
        name: m.display_name.filter(|_| ongoing),
    })
}

fn indices(list: &[u8]) -> Vec<u16> {
    list.as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_be_bytes(*b))
        .collect()
}

fn turn(m: &RouteGuidanceManeuverUpdate) -> Turn {
    Turn {
        maneuver_type: m.maneuver_type.map(u32::from),
        turn_side: m.driving_side.map(u32::from),
        junction_type: m.junction_type.map(u32::from),
        turn_angle: m.exit_angle.map(i32::from),
        after_road_name: m.after_maneuver_road_name.clone(),
    }
}

fn maneuver(m: &RouteGuidanceManeuverUpdate) -> Maneuver {
    let turn = turn(m);
    Maneuver {
        index: m.index.map_or(0, u32::from),
        maneuver_type: turn.maneuver_type,
        turn_side: turn.turn_side,
        junction_type: turn.junction_type,
        turn_angle: turn.turn_angle,
        after_road_name: turn.after_road_name,
        description: m.description.clone(),
        distance_between: m.distance_between,
        distance_between_display: m.distance_between_display.clone(),
        distance_between_units: m.distance_between_units.map(u32::from),
        junction_angles: m
            .junction_element_angles
            .iter()
            .map(|&a| i32::from(a))
            .collect(),
        linked_lane_guidance: m.linked_lane_guidance.map(u32::from),
        exit_info: m.exit_info.clone(),
        maneuver_text: None,
    }
}

fn lane_guidance(g: &LaneGuidanceInformation) -> LaneGuidance {
    LaneGuidance {
        index: g.index.map_or(0, u32::from),
        description: g.description.clone(),
        lanes: g
            .lanes
            .iter()
            .map(|l| Lane {
                index: l.index.map_or(0, u32::from),
                status: l.status.map(u32::from),
                angles: l.angles.iter().map(|&a| i32::from(a)).collect(),
                highlight_angle: l.highlight_angle.map(i32::from),
            })
            .collect(),
    }
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

    fn nav(kind: Option<HelperKind>) -> NavigationPatch {
        match kind {
            Some(HelperKind::Navigation(p)) => *p,
            other => panic!("a navigation event: {other:?}"),
        }
    }

    #[test]
    fn the_current_turn_rides_along_with_the_guidance() {
        let mut tag = EventTag::default();
        let mut exit = turn(2, 4, "Elbchaussee");
        exit.exit_angle = Some(-90);
        tag.navigation_event(&exit.encode());

        let mut update = guidance(&[2], 1);
        update.maneuver_state = Some(2);
        update.current_road_name = Some("Reeperbahn".into());
        update.distance_remaining = Some(12000);
        update.distance_to_maneuver = Some(350);
        update.distance_to_maneuver_display = Some("350".into());
        update.distance_to_maneuver_units = Some(2);
        update.source_name = Some("Maps".into());
        let v = nav(tag.navigation_event(&update.encode()));
        assert_eq!(v.status, Some(1));
        assert_eq!(v.order_type, Some(2));
        assert_eq!(v.road_name.as_deref(), Some("Reeperbahn"));
        assert_eq!(v.remain_distance, Some(350.0));
        assert_eq!(v.remain_distance_display.as_deref(), Some("350"));
        assert_eq!(v.remain_distance_units, Some(2));
        assert_eq!(v.app_name.as_deref(), Some("Maps"));
        let t = v.turn.expect("the current turn");
        assert_eq!(t.maneuver_type, Some(4));
        assert_eq!(t.turn_angle, Some(-90));
        assert_eq!(t.after_road_name.as_deref(), Some("Elbchaussee"));

        let mut replaced = turn(2, 4, "Elbchaussee");
        replaced.maneuver_type = None;
        replaced.junction_type = Some(1);
        let v = nav(tag.navigation_event(&replaced.encode()));
        let t = v.turn.expect("the current turn");
        assert_eq!(t.junction_type, Some(1));
        assert_eq!(t.maneuver_type, None, "an update replaces the turn");
        assert_eq!(t.turn_angle, None, "an update replaces the turn");
    }

    #[test]
    fn the_whole_route_is_kept_and_passed_turns_drop_off() {
        let mut tag = EventTag::default();
        for (index, kind, road) in [(0, 1, "A"), (1, 2, "B"), (2, 12, "C")] {
            let mut t = turn(index, kind, road);
            t.distance_between = Some(100 * u32::from(index));
            t.junction_element_angles = vec![-90, 45];
            let v = nav(tag.navigation_event(&t.encode()));
            assert_eq!(v.maneuvers.unwrap().len(), usize::from(index) + 1);
        }

        let v = nav(tag.navigation_event(&guidance(&[0], 1).encode()));
        let list = v.maneuvers.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].after_road_name.as_deref(), Some("B"));
        assert_eq!(list[2].distance_between, Some(200));
        assert_eq!(list[0].junction_angles, [-90, 45]);
        assert_eq!(v.current_maneuvers, Some(vec![0]));

        let v = nav(tag.navigation_event(&guidance(&[1, 2], 1).encode()));
        let list = v.maneuvers.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].index, 1);
        assert_eq!(v.current_maneuvers, Some(vec![1, 2]));

        let mut ended = guidance(&[], 0);
        ended.current_maneuver_list = None;
        let v = nav(tag.navigation_event(&ended.encode()));
        assert_eq!(v.maneuvers, Some(Vec::new()));
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
            tag.navigation_event(&lanes.encode());
        }
        let mut update = guidance(&[0], 1);
        update.lane_guidance_index = Some(1);
        update.lane_guidance_shown = Some(true);
        let v = nav(tag.navigation_event(&update.encode()));
        assert_eq!(v.lane_guidance_shown, Some(true));
        let lanes = v.lane_guidance.unwrap();
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].index, 1);
        assert_eq!(lanes[0].lanes[1].status, Some(2));
        assert_eq!(lanes[0].lanes[1].highlight_angle, Some(45));
    }

    use iap2_csm::messages::now_playing::{MediaItemAttributes, PlaybackAttributes};

    #[test]
    fn now_playing_is_a_patch() {
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
        assert_eq!(
            phone_event(&frame),
            Some(HelperKind::NowPlaying(NowPlayingPatch {
                title: Some("Song \"A\"".into()),
                artist: Some("Artist".into()),
                duration_ms: Some(180000.0),
                elapsed_ms: Some(42000.0),
                playing: Some(true),
                app: Some("Music".into()),
                ..Default::default()
            }))
        );
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
        let json = tag.tagged(wired_start_event(
            "00008120000924CE2E51A01E",
            "fe80::80a:a1ca",
        ));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({
                "type": "wired-start",
                "usbUdid": "00008120000924CE2E51A01E",
                "ip": "fe80::80a:a1ca",
                "phoneId": "0C:6A:C4:4E:F3:2A",
                "usbTransportId": "usb-1"
            })
        );
    }

    #[test]
    fn wired_marks_usb_udid() {
        let json =
            HelperEvent::from(device_event(&frame(), Some("00008120000924CE2E51A01E")).unwrap())
                .to_line();
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
        let json = tag.tagged(phone_event(&np).unwrap());
        assert!(json.contains("\"phoneId\":\"0C:6A:C4:4E:F3:2A\""), "{json}");
        assert!(json.contains("\"cid\":\"ctrl-1\""), "{json}");
        assert!(json.contains("\"usbTransportId\":\"usb-1\""), "{json}");
        assert!(json.contains("\"type\":\"nowplaying\""), "{json}");
    }

    #[test]
    fn daylight_saving_is_not_counted_twice() {
        let summer = DeviceTimeUpdate {
            seconds_since_reference_date: Some(0),
            time_zone_offset_minutes: Some(120),
            daylight_savings_offset_minutes: Some(60),
        };
        assert_eq!(
            phone_event(&summer.encode()),
            Some(HelperKind::DeviceTime {
                utc_offset_minutes: 120
            })
        );
        let winter = DeviceTimeUpdate {
            time_zone_offset_minutes: Some(60),
            daylight_savings_offset_minutes: None,
            ..summer
        };
        assert_eq!(
            phone_event(&winter.encode()),
            Some(HelperKind::DeviceTime {
                utc_offset_minutes: 60
            })
        );
    }

    #[test]
    fn a_time_without_an_offset_names_no_zone() {
        let update = DeviceTimeUpdate {
            seconds_since_reference_date: Some(0),
            time_zone_offset_minutes: None,
            daylight_savings_offset_minutes: None,
        };
        assert_eq!(phone_event(&update.encode()), None);
    }

    #[test]
    fn wireless_has_no_udid() {
        let json = HelperEvent::from(device_event(&frame(), None).unwrap()).to_line();
        assert!(json.contains("\"src\":\"bt\""));
        assert!(!json.contains("usbUdid"));
    }
}
