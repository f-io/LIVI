// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::{Config, WifiBand};
use crate::theme::Cluster;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub front: PerScreen<Front>,
    pub sessions: Sessions,
    pub now_playing: NowPlaying,
    #[serde(default)]
    pub video_playback: Option<VideoPlayback>,
    pub navigation: Navigation,
    pub system: System,
    pub devices: Vec<DeviceView>,
    pub cameras: Vec<CameraView>,
    pub mfi: Mfi,
    pub telemetry: Map<String, Value>,
    pub update: Update,
    pub cluster: Cluster,
    pub dab: Dab,
    #[serde(default)]
    pub fm: Fm,
    pub config: Config,
}

impl State {
    pub fn refresh(&mut self, section: &str, value: &Value) -> Result<(), String> {
        fn read<'a, T: Deserialize<'a>>(value: &'a Value) -> Result<T, String> {
            T::deserialize(value).map_err(|e| e.to_string())
        }
        match section {
            "front" => self.front = read(value)?,
            "sessions" => self.sessions = read(value)?,
            "nowPlaying" => self.now_playing = read(value)?,
            "videoPlayback" => self.video_playback = read(value)?,
            "navigation" => self.navigation = read(value)?,
            "system" => self.system = read(value)?,
            "devices" => self.devices = read(value)?,
            "cameras" => self.cameras = read(value)?,
            "mfi" => self.mfi = read(value)?,
            "telemetry" => self.telemetry = read(value)?,
            "update" => self.update = read(value)?,
            "cluster" => self.cluster = read(value)?,
            "dab" => self.dab = read(value)?,
            "fm" => self.fm = read(value)?,
            "config" => self.config = read(value)?,
            _ => return Err(format!("unknown section {section}")),
        }
        Ok(())
    }

    pub fn filled(partial: Value) -> Value {
        let mut full = serde_json::to_value(State::default()).unwrap_or(Value::Null);
        overlay(&mut full, partial);
        full
    }
}

fn overlay(base: &mut Value, top: Value) {
    match (base, top) {
        (Value::Object(base), Value::Object(top)) => {
            for (key, value) in top {
                match base.get_mut(&key) {
                    Some(slot) => overlay(slot, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, top) => *base = top,
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dab {
    pub receiver: Option<String>,
    pub last_station: Option<u32>,
    pub scan: Option<DabScan>,
    pub active: bool,
    pub station: Option<u32>,
    pub playing: bool,
    pub text: Option<String>,
    pub signal: DabSignal,
    pub error: Option<String>,
    pub gains_db: Vec<f64>,
    /// Last, so comparing two states stops at a changed small field before the logos.
    pub stations: Vec<DabStation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DabStation {
    pub service_id: u32,
    pub label: String,
    pub short_label: String,
    pub ensemble: String,
    pub channel: String,
    pub frequency_hz: u32,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

/// The stick is the DAB+ one, so its `receiver` in [`Dab`] says whether FM can play too.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fm {
    pub last_station: Option<u32>,
    pub scan: Option<DabScan>,
    pub active: bool,
    pub station: Option<u32>,
    pub playing: bool,
    pub name: Option<String>,
    pub text: Option<String>,
    pub signal: FmSignal,
    pub error: Option<String>,
    /// Last, so comparing two states stops at a changed small field before the logos.
    pub stations: Vec<FmStation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmStation {
    pub khz: u32,
    pub name: String,
    #[serde(default)]
    pub pi: Option<u16>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmSignal {
    pub quality: u32,
    pub stereo: bool,
    pub rds: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DabScan {
    pub channel: String,
    pub done: u32,
    pub total: u32,
    pub found: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DabSignal {
    pub locked: bool,
    pub quality: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub latest: Option<Release>,
    pub checking: bool,
    pub checked: bool,
    pub phase: UpdatePhase,
    pub received: f64,
    pub total: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub commit: String,
    pub run: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Download,
    Ready,
    Installing,
    Relaunching,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mfi {
    pub certificate: bool,
    pub identity: bool,
    pub generation: Option<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transport: Option<String>,
    pub status: DeviceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_level: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_charging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    Active,
    Available,
    #[default]
    Offline,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    pub wifi_interfaces: Vec<String>,
    pub bt_adapters: Vec<String>,
    pub interface_models: BTreeMap<String, String>,
    pub dongle: Option<DongleRadios>,
    pub link_speed: Option<LinkSpeed>,
    pub wifi_bands: Vec<WifiBand>,
    pub wifi_channels: Vec<u32>,
    pub wifi_countries: Vec<String>,
    pub displays: Vec<Display>,
    pub displays_settable: bool,
    pub audio_sinks: Vec<AudioDevice>,
    pub audio_sources: Vec<AudioDevice>,
    pub start_page: String,
    pub fonts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DongleRadios {
    pub wifi: bool,
    pub bt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpeed {
    pub down_mbps: f64,
    pub up_mbps: f64,
    pub down_rate: f64,
    pub up_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigation {
    pub active: Option<bool>,
    pub order_type: Option<u32>,
    pub road_name: Option<String>,
    pub after_road_name: Option<String>,
    pub destination_name: Option<String>,
    pub time_to_destination: Option<f64>,
    pub distance_to_destination: Option<f64>,
    pub remain_distance: Option<f64>,
    pub maneuver_type: Option<u32>,
    pub turn_side: Option<u32>,
    pub junction_type: Option<u32>,
    pub turn_angle: Option<i32>,
    pub eta: Option<f64>,
    pub eta_text: Option<String>,
    pub app_name: Option<String>,
    pub image: Option<String>,
    pub maneuver_text: Option<String>,
    pub maneuver_distance_text: Option<String>,
    pub destination_distance_text: Option<String>,
    pub time_left_text: Option<String>,
    pub distance_to_destination_display: Option<String>,
    pub distance_to_destination_units: Option<u32>,
    pub remain_distance_display: Option<String>,
    pub remain_distance_units: Option<u32>,
    pub maneuver_count: Option<u32>,
    pub visible_in_app: Option<bool>,
    pub lane_guidance_index: Option<u32>,
    pub lane_guidance_count: Option<u32>,
    pub lane_guidance_shown: Option<bool>,
    pub source_supports_route_guidance: Option<bool>,
    pub destination_time_zone_offset_minutes: Option<i32>,
    #[serde(default)]
    pub maneuvers: Vec<Maneuver>,
    #[serde(default)]
    pub current_maneuvers: Vec<u32>,
    #[serde(default)]
    pub lane_guidance: Vec<LaneGuidance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Maneuver {
    pub index: u32,
    pub maneuver_type: Option<u32>,
    pub turn_side: Option<u32>,
    pub junction_type: Option<u32>,
    pub turn_angle: Option<i32>,
    pub after_road_name: Option<String>,
    pub description: Option<String>,
    pub distance_between: Option<u32>,
    pub distance_between_display: Option<String>,
    pub distance_between_units: Option<u32>,
    pub junction_angles: Vec<i32>,
    pub linked_lane_guidance: Option<u32>,
    pub exit_info: Option<String>,
    pub maneuver_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LaneGuidance {
    pub index: u32,
    pub description: Option<String>,
    pub lanes: Vec<Lane>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lane {
    pub index: u32,
    pub status: Option<u32>,
    pub angles: Vec<i32>,
    pub highlight_angle: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub app: Option<String>,
    pub duration_ms: Option<f64>,
    pub elapsed_ms: Option<f64>,
    pub playing: Option<bool>,
    pub artwork: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlayback {
    pub playing: bool,
    pub position_ms: f64,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sessions {
    pub active: Option<Protocol>,
    pub video: bool,
    pub position: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Carplay,
    Androidauto,
    Airplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Screen {
    Main,
    Cluster,
    Aux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Front {
    #[default]
    Livi,
    Projection,
    Cluster,
    Camera,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub connector: String,
    pub id: String,
    pub panel: String,
    pub screen: Option<Screen>,
    pub current: Option<String>,
    pub modes: Vec<String>,
    pub touch: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PerScreen<T> {
    pub main: T,
    pub cluster: T,
    pub aux: T,
}

impl<T> PerScreen<T> {
    pub fn get(&self, screen: Screen) -> &T {
        match screen {
            Screen::Main => &self.main,
            Screen::Cluster => &self.cluster,
            Screen::Aux => &self.aux,
        }
    }

    pub fn get_mut(&mut self, screen: Screen) -> &mut T {
        match screen {
            Screen::Main => &mut self.main,
            Screen::Cluster => &mut self.cluster,
            Screen::Aux => &mut self.aux,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_reads_back_what_the_core_wrote() {
        let mut written = State::default();
        written.sessions.total = 2;
        written.config.language = "de".into();
        written.video_playback = Some(VideoPlayback::default());
        let Value::Object(sections) = serde_json::to_value(&written).unwrap() else {
            panic!("a state is an object");
        };
        let mut read = State::default();
        for (section, value) in &sections {
            read.refresh(section, value).unwrap();
        }
        assert_eq!(read, written);
    }

    #[test]
    fn a_broken_section_is_refused_and_the_rest_kept() {
        let mut state = State::default();
        assert!(
            state
                .refresh("sessions", &serde_json::json!({ "total": "two" }))
                .is_err()
        );
        assert!(state.refresh("weather", &Value::Null).is_err());
        assert_eq!(state, State::default());
    }

    #[test]
    fn a_partial_state_is_filled_with_the_defaults() {
        let filled = State::filled(serde_json::json!({
            "config": { "language": "de" },
            "sessions": { "active": "carplay" }
        }));
        let state: State = serde_json::from_value(filled).unwrap();
        assert_eq!(state.config.language, "de");
        assert_eq!(
            state.config.projection_width,
            crate::config::defaults().projection_width
        );
        assert_eq!(state.sessions.active, Some(Protocol::Carplay));
    }

    #[test]
    fn per_screen_addresses_each_screen() {
        let mut fronts = PerScreen {
            main: Front::Livi,
            cluster: Front::Livi,
            aux: Front::Livi,
        };
        *fronts.get_mut(Screen::Cluster) = Front::Cluster;
        assert_eq!(*fronts.get(Screen::Main), Front::Livi);
        assert_eq!(*fronts.get(Screen::Cluster), Front::Cluster);
        assert_eq!(*fronts.get(Screen::Aux), Front::Livi);
    }

    #[test]
    fn fronts_are_lower_case_on_the_wire() {
        let fronts = PerScreen {
            main: Front::Projection,
            cluster: Front::Cluster,
            aux: Front::Camera,
        };
        assert_eq!(
            serde_json::to_string(&fronts).unwrap(),
            r#"{"main":"projection","cluster":"cluster","aux":"camera"}"#
        );
    }

    #[test]
    fn now_playing_says_null_for_what_the_phone_did_not_tell() {
        let np = NowPlaying {
            title: Some("Song".into()),
            playing: Some(true),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&np).unwrap(),
            serde_json::json!({
                "title": "Song", "artist": null, "album": null, "app": null,
                "durationMs": null, "elapsedMs": null, "playing": true, "artwork": null
            })
        );
    }

    #[test]
    fn sessions_name_the_protocol_as_the_ui_does() {
        let sessions = Sessions {
            active: Some(Protocol::Androidauto),
            video: true,
            position: 1,
            total: 2,
        };
        assert_eq!(
            serde_json::to_string(&sessions).unwrap(),
            r#"{"active":"androidauto","video":true,"position":1,"total":2}"#
        );
        assert_eq!(
            serde_json::to_string(&Sessions::default()).unwrap(),
            r#"{"active":null,"video":false,"position":0,"total":0}"#
        );
    }
}
