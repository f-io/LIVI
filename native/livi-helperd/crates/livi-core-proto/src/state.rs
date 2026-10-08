//! The state says what to show, never why.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::config::{Config, WifiBand};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct State {
    pub front: PerScreen<Front>,
    pub sessions: Sessions,
    pub now_playing: NowPlaying,
    pub navigation: Navigation,
    pub system: System,
    /// Those in a session first.
    pub devices: Vec<DeviceView>,
    /// What the car and outside producers pushed, merged into one snapshot.
    #[ts(type = "Record<string, unknown>")]
    pub telemetry: Map<String, Value>,
    pub update: Update,
    pub config: Config,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct Update {
    /// On the chosen channel, as the last check found it.
    pub latest: Option<Release>,
    pub checking: bool,
    /// Also true when the feed did not answer.
    pub checked: bool,
    pub phase: UpdatePhase,
    pub received: f64,
    /// 0 while the server did not say how big the download is.
    pub total: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct Release {
    pub version: String,
    pub commit: String,
    /// The build number of a nightly.
    pub run: String,
    /// None when the release has no build for this machine.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "contract.ts")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Download,
    Ready,
    Installing,
    Relaunching,
    Error,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct DeviceView {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub protocol: Option<Protocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_transport: Option<String>,
    pub status: DeviceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub battery_level: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub battery_charging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub signal_strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub carrier_name: Option<String>,
    /// Place among the sessions, counted from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session: Option<u32>,
}

/// Active projects, available is held or on the access point, offline is
/// only remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "contract.ts")]
pub enum DeviceStatus {
    Active,
    Available,
    #[default]
    Offline,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct System {
    /// The LIVI Link is among them while it answers.
    pub wifi_interfaces: Vec<String>,
    pub bt_adapters: Vec<String>,
    /// None while the LIVI Link does not answer.
    pub dongle: Option<DongleRadios>,
    /// The phone's Wi-Fi link on the access point.
    pub link_speed: Option<LinkSpeed>,
    /// The bands an access point on the chosen interface can run on.
    pub wifi_bands: Vec<WifiBand>,
    /// Allowed in the chosen band and country.
    pub wifi_channels: Vec<u32>,
    pub wifi_countries: Vec<String>,
    /// The panel's modes as "WIDTHxHEIGHT", largest first.
    pub display_modes: Vec<String>,
    /// Under GNOME the modes are only shown, LIVI cannot put the panel into one.
    pub display_mode_settable: bool,
    pub audio_sinks: Vec<AudioDevice>,
    pub audio_sources: Vec<AudioDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// A paired Bluetooth device that is not connected right now.
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub struct DongleRadios {
    pub wifi: bool,
    pub bt: bool,
}

/// Down is phone to car, the stream, up is car to phone, touch and mic.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct LinkSpeed {
    pub down_mbps: f64,
    pub up_mbps: f64,
    /// The negotiated bitrates, 0 until a phone is on the air.
    pub down_rate: f64,
    pub up_rate: f64,
}

/// The route guidance of the phone in front, as far as it told.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct Navigation {
    pub active: Option<bool>,
    pub order_type: Option<u32>,
    pub road_name: Option<String>,
    pub after_road_name: Option<String>,
    pub destination_name: Option<String>,
    pub time_to_destination: Option<f64>,
    pub distance_to_destination: Option<f64>,
    /// To the next maneuver.
    pub remain_distance: Option<f64>,
    pub maneuver_type: Option<u32>,
    pub turn_side: Option<u32>,
    pub junction_type: Option<u32>,
    pub turn_angle: Option<i32>,
    /// Arrival in Unix seconds, the UI shows it on its own clock.
    pub eta: Option<f64>,
    /// Arrival as the phone wrote it, for a phone that sends no time.
    pub eta_text: Option<String>,
    pub app_name: Option<String>,
    /// The phone's own picture of the next maneuver, base64.
    pub image: Option<String>,
    /// In the UI's language.
    pub maneuver_text: Option<String>,
    pub maneuver_distance_text: Option<String>,
    pub destination_distance_text: Option<String>,
    pub time_left_text: Option<String>,
}

/// What the phone in front plays, as far as it told.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub app: Option<String>,
    pub duration_ms: Option<f64>,
    /// Where the track was when the phone last said, the UI counts on from there.
    pub elapsed_ms: Option<f64>,
    pub playing: Option<bool>,
    /// Base64 image data.
    pub artwork: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct Sessions {
    pub active: Option<Protocol>,
    /// Counted from 1, 0 without an active session.
    pub position: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "contract.ts")]
pub enum Protocol {
    Carplay,
    Androidauto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum Screen {
    Main,
    Dash,
    Aux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum Front {
    /// Which page is up to the UI.
    Livi,
    Projection,
    Cluster,
    Camera,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub struct PerScreen<T> {
    pub main: T,
    pub dash: T,
    pub aux: T,
}

impl<T> PerScreen<T> {
    pub fn get(&self, screen: Screen) -> &T {
        match screen {
            Screen::Main => &self.main,
            Screen::Dash => &self.dash,
            Screen::Aux => &self.aux,
        }
    }

    pub fn get_mut(&mut self, screen: Screen) -> &mut T {
        match screen {
            Screen::Main => &mut self.main,
            Screen::Dash => &mut self.dash,
            Screen::Aux => &mut self.aux,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_screen_addresses_each_screen() {
        let mut fronts = PerScreen { main: Front::Livi, dash: Front::Livi, aux: Front::Livi };
        *fronts.get_mut(Screen::Dash) = Front::Cluster;
        assert_eq!(*fronts.get(Screen::Main), Front::Livi);
        assert_eq!(*fronts.get(Screen::Dash), Front::Cluster);
        assert_eq!(*fronts.get(Screen::Aux), Front::Livi);
    }

    #[test]
    fn fronts_are_lower_case_on_the_wire() {
        let fronts =
            PerScreen { main: Front::Projection, dash: Front::Cluster, aux: Front::Camera };
        assert_eq!(
            serde_json::to_string(&fronts).unwrap(),
            r#"{"main":"projection","dash":"cluster","aux":"camera"}"#
        );
    }

    #[test]
    fn now_playing_says_null_for_what_the_phone_did_not_tell() {
        let np =
            NowPlaying { title: Some("Song".into()), playing: Some(true), ..Default::default() };
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
        let sessions = Sessions { active: Some(Protocol::Androidauto), position: 1, total: 2 };
        assert_eq!(
            serde_json::to_string(&sessions).unwrap(),
            r#"{"active":"androidauto","position":1,"total":2}"#
        );
        assert_eq!(
            serde_json::to_string(&Sessions::default()).unwrap(),
            r#"{"active":null,"position":0,"total":0}"#
        );
    }
}
