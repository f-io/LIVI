//! Fields only the UI reads live here as well, so the settings file has one owner.

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use ts_rs::TS;

use crate::state::PerScreen;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts", optional_fields)]
pub struct Config {
    pub debug_logging: bool,

    pub wireless_aa_enabled: bool,
    pub wireless_cp_enabled: bool,

    pub wifi_password: String,
    pub bt_adapter: String,
    pub wifi_interface: String,
    pub wifi_dedicated_interface: bool,
    pub wifi_type: WifiBand,
    pub wifi_channel: u32,
    pub wifi_channel_width: u32,
    pub country: String,

    pub car_play_source_version: String,
    pub car_play_mfi_i2c_bus: u32,
    /// -1 when the coprocessor has no power pin.
    pub car_play_mfi_power_gpio: i32,

    pub gps_enabled: bool,
    pub gps_device: String,
    pub gps_baud_rate: u32,
    /// From the last GPS fix, applied at startup.
    pub timezone: String,

    pub projection_width: u32,
    pub projection_height: u32,
    pub projection_fps: u32,
    pub projection_dpi: u32,
    pub projection_view_area_top: u32,
    pub projection_view_area_bottom: u32,
    pub projection_view_area_left: u32,
    pub projection_view_area_right: u32,
    pub projection_safe_area_top: u32,
    pub projection_safe_area_bottom: u32,
    pub projection_safe_area_left: u32,
    pub projection_safe_area_right: u32,
    pub projection_safe_area_draw_outside: bool,

    pub cluster_width: u32,
    pub cluster_height: u32,
    pub cluster_fps: u32,
    pub cluster_dpi: u32,
    pub cluster_view_area_top: u32,
    pub cluster_view_area_bottom: u32,
    pub cluster_view_area_left: u32,
    pub cluster_view_area_right: u32,
    pub cluster_safe_area_top: u32,
    pub cluster_safe_area_bottom: u32,
    pub cluster_safe_area_left: u32,
    pub cluster_safe_area_right: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected_aa_bt_mac: Option<String>,

    pub dark_mode: bool,
    pub overlay_messages: bool,
    pub display_brightness: f64,
    pub display_brightness_auto: bool,
    /// The Wi-Fi AP, Bluetooth and head-unit name, at most 20 characters.
    pub car_name: String,
    pub oem_name: String,
    pub hand: HandDriveType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_type: Option<CarType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ev_connector_types: Option<Vec<EvConnectorType>>,
    pub max_speed_kph: u32,
    /// Speed between two numbered marks on the dash, in the unit shown.
    pub speed_scale_step: u32,
    pub max_rpm: u32,
    /// 0 for none.
    pub redline_rpm: u32,
    pub speed_unit: SpeedUnit,
    pub temperature_unit: TemperatureUnit,

    #[ts(type = "0 | 1")]
    pub sampling_frequency: u8,
    pub disable_audio_output: bool,
    pub hu_volume: f64,
    pub hu_volume_link_system: bool,
    pub audio_volume: f64,
    pub nav_volume: f64,
    pub voice_assistant_volume: f64,
    pub call_volume: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_sounds_volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_output_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_output_device_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input_device_label: Option<String>,
    pub visual_audio_delay_ms: u32,

    pub auto_conn: bool,
    pub auto_switch_on_reverse: bool,

    pub start_page: String,
    pub language: String,
    pub kiosk: PerScreen<bool>,
    pub ui_zoom_percent: u32,
    pub appearance_mode: AppearanceMode,

    /// "WIDTHxHEIGHT", empty leaves the panel at the mode it came up in.
    pub display_mode: String,

    pub display_gamma: f64,
    pub display_contrast: f64,
    pub display_color_r: f64,
    pub display_color_g: f64,
    pub display_color_b: f64,

    pub camera_id: String,
    pub camera: PerScreen<bool>,
    pub camera_mirror: bool,
    #[ts(type = "0 | 90 | 180 | 270")]
    pub camera_rotation: u16,
    pub media: PerScreen<bool>,
    pub dashboards: Dashboards,
    pub custom: PerScreen<bool>,
    pub custom_url: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_screen_bounds: Option<WindowBounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_screen_bounds: Option<WindowBounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aux_screen_bounds: Option<WindowBounds>,
    pub main_screen_width: u32,
    pub main_screen_height: u32,
    pub dash_screen_active: bool,
    pub dash_screen_width: u32,
    pub dash_screen_height: u32,
    pub aux_screen_active: bool,
    pub aux_screen_width: u32,
    pub aux_screen_height: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_gps: Option<LastKnownGps>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_color_dark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_color_light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_color_light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_color_dark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color_dark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color_light: Option<String>,

    /// Override for the logo on the CarPlay tile that leads back to LIVI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carplay_icon120: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carplay_icon180: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carplay_icon256: Option<String>,

    pub update_nightly: bool,
    pub dismissed_packages: Vec<String>,

    pub bindings: KeyBindings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub enum WifiBand {
    #[serde(rename = "2.4ghz")]
    Ghz24,
    #[serde(rename = "5ghz")]
    Ghz5,
    #[serde(rename = "6ghz")]
    Ghz6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum AppearanceMode {
    Auto,
    Night,
    Day,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum SpeedUnit {
    Kmh,
    Mph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr, TS)]
#[repr(u8)]
#[ts(export_to = "contract.ts", repr(enum))]
pub enum HandDriveType {
    #[ts(rename = "LHD")]
    Lhd = 0,
    #[ts(rename = "RHD")]
    Rhd = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr, TS)]
#[repr(u8)]
#[ts(export_to = "contract.ts", repr(enum))]
pub enum CarType {
    Unknown = 0,
    Gasoline = 1,
    DieselWinter = 3,
    Diesel = 4,
    Biodiesel = 5,
    E85 = 6,
    #[ts(rename = "LPG")]
    Lpg = 7,
    #[ts(rename = "CNG")]
    Cng = 8,
    #[ts(rename = "LNG")]
    Lng = 9,
    Electric = 10,
    Hydrogen = 11,
    Other = 12,
    HybridGasoline = 101,
    HybridDiesel = 102,
}

impl CarType {
    /// The names the UI uses, in any case.
    pub fn from_name(name: &str) -> Option<Self> {
        const ALL: [(&str, CarType); 13] = [
            ("Gasoline", CarType::Gasoline),
            ("DieselWinter", CarType::DieselWinter),
            ("Diesel", CarType::Diesel),
            ("Biodiesel", CarType::Biodiesel),
            ("E85", CarType::E85),
            ("LPG", CarType::Lpg),
            ("CNG", CarType::Cng),
            ("LNG", CarType::Lng),
            ("Electric", CarType::Electric),
            ("Hydrogen", CarType::Hydrogen),
            ("Other", CarType::Other),
            ("HybridGasoline", CarType::HybridGasoline),
            ("HybridDiesel", CarType::HybridDiesel),
        ];
        ALL.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, t)| *t)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr, TS)]
#[repr(u8)]
#[ts(export_to = "contract.ts", repr(enum))]
pub enum EvConnectorType {
    Unknown = 0,
    J1772 = 1,
    Mennekes = 2,
    Chademo = 3,
    Combo1 = 4,
    Combo2 = 5,
    TeslaSupercharger = 8,
    Gbt = 9,
    Other = 101,
}

impl EvConnectorType {
    /// The names the UI uses, in any case.
    pub fn from_name(name: &str) -> Option<Self> {
        const ALL: [(&str, EvConnectorType); 8] = [
            ("J1772", EvConnectorType::J1772),
            ("Mennekes", EvConnectorType::Mennekes),
            ("Chademo", EvConnectorType::Chademo),
            ("Combo1", EvConnectorType::Combo1),
            ("Combo2", EvConnectorType::Combo2),
            ("TeslaSupercharger", EvConnectorType::TeslaSupercharger),
            ("Gbt", EvConnectorType::Gbt),
            ("Other", EvConnectorType::Other),
        ];
        ALL.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, t)| *t)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub struct Dashboards {
    pub dash1: DashboardSlot,
    pub dash2: DashboardSlot,
    pub dash3: DashboardSlot,
    pub dash4: DashboardSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub struct DashboardSlot {
    pub main: bool,
    pub dash: bool,
    pub aux: bool,
    pub pos: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts")]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "contract.ts", optional_fields)]
pub struct LastKnownGps {
    pub lat: f64,
    pub lng: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<f64>,
    /// Milliseconds since the epoch.
    pub ts: u64,
}

/// Each value is a KeyboardEvent.code, empty when unbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub struct KeyBindings {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub select_up: String,
    pub select_down: String,
    pub back: String,

    pub knob_left: String,
    pub knob_right: String,
    pub knob_up: String,
    pub knob_down: String,

    pub home: String,
    pub cycle_session: String,
    pub play_pause: String,
    pub play: String,
    pub pause: String,
    pub next: String,
    pub prev: String,

    pub accept_phone: String,
    pub reject_phone: String,
    pub phone_key0: String,
    pub phone_key1: String,
    pub phone_key2: String,
    pub phone_key3: String,
    pub phone_key4: String,
    pub phone_key5: String,
    pub phone_key6: String,
    pub phone_key7: String,
    pub phone_key8: String,
    pub phone_key9: String,
    pub phone_key_star: String,
    pub phone_key_hash: String,
    pub phone_key_hook_switch: String,

    pub voice_assistant: String,
    pub voice_assistant_release: String,
}

pub fn defaults() -> Config {
    let off = PerScreen { main: false, dash: false, aux: false };
    let main_only = PerScreen { main: true, dash: false, aux: false };
    let slot = |pos| DashboardSlot { main: true, dash: false, aux: false, pos };
    let no_bounds = Some(WindowBounds { x: 0, y: 0, width: 0, height: 0 });
    let empty = || Some(String::new());
    Config {
        debug_logging: false,
        wireless_aa_enabled: false,
        wireless_cp_enabled: false,
        wifi_password: "12345678".into(),
        bt_adapter: "hci0".into(),
        wifi_interface: "wlan0".into(),
        wifi_dedicated_interface: false,
        wifi_type: WifiBand::Ghz5,
        wifi_channel: 36,
        wifi_channel_width: 40,
        country: "DE".into(),
        car_play_source_version: "950.7.1".into(),
        car_play_mfi_i2c_bus: 2,
        car_play_mfi_power_gpio: -1,
        gps_enabled: false,
        gps_device: "/dev/ttyAMA0".into(),
        gps_baud_rate: 38400,
        timezone: String::new(),
        projection_width: 1280,
        projection_height: 720,
        projection_fps: 60,
        projection_dpi: 0,
        projection_view_area_top: 0,
        projection_view_area_bottom: 0,
        projection_view_area_left: 0,
        projection_view_area_right: 0,
        projection_safe_area_top: 0,
        projection_safe_area_bottom: 0,
        projection_safe_area_left: 0,
        projection_safe_area_right: 0,
        projection_safe_area_draw_outside: true,
        cluster_width: 1280,
        cluster_height: 720,
        cluster_fps: 60,
        cluster_dpi: 0,
        cluster_view_area_top: 0,
        cluster_view_area_bottom: 0,
        cluster_view_area_left: 0,
        cluster_view_area_right: 0,
        cluster_safe_area_top: 60,
        cluster_safe_area_bottom: 10,
        cluster_safe_area_left: 300,
        cluster_safe_area_right: 300,
        last_connected_aa_bt_mac: empty(),
        dark_mode: true,
        overlay_messages: true,
        display_brightness: 1.0,
        display_brightness_auto: true,
        car_name: "LIVI".into(),
        oem_name: "App".into(),
        hand: HandDriveType::Lhd,
        car_type: Some(CarType::Gasoline),
        ev_connector_types: Some(Vec::new()),
        max_speed_kph: 200,
        speed_scale_step: 40,
        max_rpm: 5000,
        redline_rpm: 4500,
        speed_unit: SpeedUnit::Kmh,
        temperature_unit: TemperatureUnit::Celsius,
        sampling_frequency: 1,
        disable_audio_output: false,
        hu_volume: 0.95,
        hu_volume_link_system: true,
        audio_volume: 0.95,
        nav_volume: 0.95,
        voice_assistant_volume: 0.95,
        call_volume: 0.95,
        system_sounds_volume: Some(0.75),
        audio_output_device: empty(),
        audio_output_device_label: empty(),
        audio_input_device: empty(),
        audio_input_device_label: empty(),
        visual_audio_delay_ms: 0,
        auto_conn: true,
        auto_switch_on_reverse: true,
        start_page: "/".into(),
        language: "en".into(),
        kiosk: off,
        ui_zoom_percent: 100,
        appearance_mode: AppearanceMode::Auto,
        display_mode: String::new(),
        display_gamma: 1.0,
        display_contrast: 1.0,
        display_color_r: 1.0,
        display_color_g: 1.0,
        display_color_b: 1.0,
        camera_id: String::new(),
        camera: main_only,
        camera_mirror: false,
        camera_rotation: 0,
        media: main_only,
        dashboards: Dashboards { dash1: slot(1), dash2: slot(2), dash3: slot(3), dash4: slot(4) },
        custom: off,
        custom_url: String::new(),
        main_screen_bounds: no_bounds,
        dash_screen_bounds: no_bounds,
        aux_screen_bounds: no_bounds,
        main_screen_width: 1280,
        main_screen_height: 720,
        dash_screen_active: false,
        dash_screen_width: 1280,
        dash_screen_height: 720,
        aux_screen_active: false,
        aux_screen_width: 800,
        aux_screen_height: 480,
        last_known_gps: None,
        primary_color_dark: empty(),
        primary_color_light: empty(),
        highlight_color_light: empty(),
        highlight_color_dark: empty(),
        background_color_dark: empty(),
        background_color_light: empty(),
        carplay_icon120: empty(),
        carplay_icon180: empty(),
        carplay_icon256: empty(),
        update_nightly: false,
        dismissed_packages: Vec::new(),
        bindings: default_bindings(),
    }
}

fn default_bindings() -> KeyBindings {
    let key = |code: &str| code.to_string();
    KeyBindings {
        up: key("ArrowUp"),
        down: key("ArrowDown"),
        left: key("ArrowLeft"),
        right: key("ArrowRight"),
        select_up: key(""),
        select_down: key("Enter"),
        back: key("Backspace"),
        knob_left: key(""),
        knob_right: key(""),
        knob_up: key(""),
        knob_down: key(""),
        home: key("KeyH"),
        cycle_session: key("KeyS"),
        play_pause: key("KeyP"),
        play: key(""),
        pause: key(""),
        next: key("KeyN"),
        prev: key("KeyB"),
        accept_phone: key("KeyA"),
        reject_phone: key("KeyR"),
        phone_key0: key("Digit0"),
        phone_key1: key("Digit1"),
        phone_key2: key("Digit2"),
        phone_key3: key("Digit3"),
        phone_key4: key("Digit4"),
        phone_key5: key("Digit5"),
        phone_key6: key("Digit6"),
        phone_key7: key("Digit7"),
        phone_key8: key("Digit8"),
        phone_key9: key("Digit9"),
        phone_key_star: key(""),
        phone_key_hash: key(""),
        phone_key_hook_switch: key(""),
        voice_assistant: key("KeyV"),
        voice_assistant_release: key(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_telemetry_match_in_any_case() {
        assert_eq!(CarType::from_name(" hybriddiesel "), Some(CarType::HybridDiesel));
        assert_eq!(CarType::from_name("LPG"), Some(CarType::Lpg));
        assert_eq!(CarType::from_name("Unknown"), None);
        assert_eq!(EvConnectorType::from_name("combo2"), Some(EvConnectorType::Combo2));
        assert_eq!(EvConnectorType::from_name("Type 2"), None);
    }
}
