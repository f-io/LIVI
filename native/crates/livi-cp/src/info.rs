// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::bplist::{Value, dict};
use crate::hid;

const STREAM_TYPE_MAIN_SCREEN: u64 = 110;
const STREAM_TYPE_ALT_SCREEN: u64 = 111;
const DISPLAY_FEATURE_KNOBS: u64 = 0x02;
const DISPLAY_FEATURE_HIGH_FIDELITY_TOUCH: u64 = 0x08;
const PRIMARY_INPUT_KNOBS: u64 = 3;
const APPEARANCE_DARK: u64 = 1;
const APPEARANCE_AUTOMATIC: u64 = 0;

const CARPLAY_FEATURES: u64 = 0x61_5653_aee2;
const CARPLAY_AUDIO_FEATURES: u64 = 0x100_0454_0a00;

pub const MAIN_UUID: &str = "b7e6c5a0-1111-4000-8000-000000000001";
pub const ALT_UUID: &str = "b7e6c5a0-2222-4000-8000-000000000002";
const CLUSTER_UID: &str = "b7e6c5a0-3333-4000-8000-000000000003";

const RESOURCE_SCREEN: u64 = 1;
const RESOURCE_AUDIO: u64 = 2;
const TRANSFER_TAKE: u64 = 1;
const PRIORITY_NICE_TO_HAVE: u64 = 100;
const CONSTRAINT_ANYTIME: u64 = 100;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Insets {
    pub top: i64,
    pub bottom: i64,
    pub left: i64,
    pub right: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayConfig {
    pub width_pixels: i64,
    pub height_pixels: i64,
    pub width_physical_mm: Option<i64>,
    pub height_physical_mm: Option<i64>,
    pub fps: Option<i64>,
    pub primary_input_device: Option<i64>,
    pub view_area: Option<Insets>,
    pub safe_area: Option<Insets>,
    pub safe_area_draw_outside: Option<bool>,
    pub status_bar_edge: Option<u64>,
    pub initial_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width_pixels: u64,
    pub height_pixels: u64,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InfoConfig {
    pub device_name: String,
    pub device_id: String,
    pub bt_mac: String,
    pub source_version: String,
    pub hevc: bool,
    pub main: DisplayConfig,
    pub cluster: Option<DisplayConfig>,
    pub entertainment_sample_rate: u32,
    pub disable_audio_output: bool,
    pub oem_label: String,
    pub icons: Vec<Icon>,
    pub right_hand_drive: bool,
    pub theme_displays: Option<ThemeDisplays>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeDisplays {
    pub main: String,
    pub cluster: String,
}

fn num(v: i64) -> Value {
    u64::try_from(v)
        .map(Value::Int)
        .unwrap_or(Value::Real(v as f64))
}

fn resource(id: u64) -> Value {
    dict([
        ("resourceID", Value::Int(id)),
        ("transferType", Value::Int(TRANSFER_TAKE)),
        ("transferPriority", Value::Int(PRIORITY_NICE_TO_HAVE)),
        ("takeConstraint", Value::Int(CONSTRAINT_ANYTIME)),
        ("borrowConstraint", Value::Int(CONSTRAINT_ANYTIME)),
        ("unborrowConstraint", Value::Int(CONSTRAINT_ANYTIME)),
    ])
}

fn modes() -> Value {
    dict([
        (
            "resources",
            Value::Array(vec![resource(RESOURCE_SCREEN), resource(RESOURCE_AUDIO)]),
        ),
        (
            "appStates",
            Value::Array(vec![
                dict([("appStateID", Value::Int(2)), ("state", Value::Bool(false))]),
                dict([("appStateID", Value::Int(1)), ("speechMode", num(-1))]),
                dict([("appStateID", Value::Int(3)), ("state", Value::Bool(false))]),
            ]),
        ),
    ])
}

fn audio_latencies() -> Value {
    let base = |kind: u64, audio_type: Option<&str>| {
        let mut entry = vec![
            ("type".to_string(), Value::Int(kind)),
            ("inputLatencyMicros".to_string(), Value::Int(0)),
            ("outputLatencyMicros".to_string(), Value::Int(0)),
        ];
        if let Some(t) = audio_type {
            entry.push(("audioType".to_string(), Value::String(t.into())));
        }
        Value::Dict(entry)
    };
    Value::Array(vec![
        base(100, None),
        base(100, Some("default")),
        base(100, Some("media")),
        base(100, Some("telephony")),
        base(100, Some("speechRecognition")),
        base(100, Some("alert")),
        base(101, None),
        base(101, Some("default")),
        base(102, Some("default")),
    ])
}

fn audio_formats(entertainment_rate: u32) -> Value {
    let f = |kind: u64, audio_type: &str, output: u64, input: Option<u64>| {
        let mut entry = vec![
            ("type".to_string(), Value::Int(kind)),
            ("audioType".to_string(), Value::String(audio_type.into())),
            ("audioOutputFormats".to_string(), Value::Int(output)),
        ];
        if let Some(input) = input {
            entry.push(("audioInputFormats".to_string(), Value::Int(input)));
        }
        Value::Dict(entry)
    };
    let is48 = entertainment_rate == 48000;
    let pcm_voice = 0x3fc;
    let pcm = pcm_voice | if is48 { 0xc000 } else { 0xc00 };
    let pcm_mono = 0x154 | if is48 { 0x4000 } else { 0x400 };
    let pcm_media = if is48 { 0x8000 } else { 0x800 };
    let opus = 0x7000_0000;
    let aac_lc = if is48 { 0x80_0000 } else { 0x40_0000 };
    Value::Array(vec![
        f(100, "compatibility", pcm, Some(pcm_mono)),
        f(101, "compatibility", pcm, None),
        f(100, "default", pcm | opus, Some(pcm_mono | opus)),
        f(100, "alert", pcm | opus, None),
        f(100, "media", pcm_media, None),
        f(100, "telephony", pcm_mono | opus, Some(pcm_mono | opus)),
        f(
            100,
            "speechRecognition",
            pcm_mono | opus,
            Some(pcm_mono | opus),
        ),
        f(101, "default", pcm | opus, None),
        f(102, "media", aac_lc, None),
    ])
}

fn area(d: &DisplayConfig) -> Option<Value> {
    let va = d.view_area?;
    let (w, h) = (d.width_pixels, d.height_pixels);
    let mut view = vec![
        ("widthPixels".to_string(), num(w - va.left - va.right)),
        ("heightPixels".to_string(), num(h - va.top - va.bottom)),
        ("originXPixels".to_string(), num(va.left)),
        ("originYPixels".to_string(), num(va.top)),
    ];
    if let Some(sa) = d.safe_area {
        let mut safe = vec![
            ("widthPixels".to_string(), num(w - sa.left - sa.right)),
            ("heightPixels".to_string(), num(h - sa.top - sa.bottom)),
            ("originXPixels".to_string(), num(sa.left)),
            ("originYPixels".to_string(), num(sa.top)),
        ];
        if let Some(outside) = d.safe_area_draw_outside {
            safe.push(("drawUIOutsideSafeArea".to_string(), Value::Bool(outside)));
        }
        view.push(("safeArea".to_string(), Value::Dict(safe)));
    }
    if let Some(edge) = d.status_bar_edge {
        view.push(("viewAreaStatusBarEdge".to_string(), Value::Int(edge)));
    }
    view.push(("viewAreaTransitionControl".to_string(), Value::Bool(false)));
    view.push((
        "viewAreaSupportsFocusTransfer".to_string(),
        Value::Bool(false),
    ));
    Some(Value::Dict(view))
}

const FALLBACK_DPI: f64 = 200.0;

pub fn mm_at_dpi(pixels: i64, dpi: f64) -> i64 {
    (hid::round_half_up(pixels as f64 * 25.4 / dpi) as i64).max(1)
}

fn display(d: &DisplayConfig, kind: u64, uuid: &str) -> Value {
    let width_physical = d
        .width_physical_mm
        .unwrap_or_else(|| mm_at_dpi(d.width_pixels, FALLBACK_DPI));
    let height_physical = d.height_physical_mm.unwrap_or_else(|| {
        let h = width_physical as f64 * d.height_pixels as f64 / d.width_pixels.max(1) as f64;
        (hid::round_half_up(h) as i64).max(1)
    });
    let mut entry = vec![
        ("uuid".to_string(), Value::String(uuid.into())),
        ("type".to_string(), Value::Int(kind)),
        ("maxFPS".to_string(), num(d.fps.unwrap_or(60))),
        ("widthPixels".to_string(), num(d.width_pixels)),
        ("heightPixels".to_string(), num(d.height_pixels)),
        ("widthPhysical".to_string(), num(width_physical)),
        ("heightPhysical".to_string(), num(height_physical)),
        (
            "features".to_string(),
            Value::Int(DISPLAY_FEATURE_HIGH_FIDELITY_TOUCH | DISPLAY_FEATURE_KNOBS),
        ),
        (
            "primaryInputDevice".to_string(),
            num(d.primary_input_device.unwrap_or(PRIMARY_INPUT_KNOBS as i64)),
        ),
        ("cornerMasks".to_string(), Value::Bool(false)),
        ("adjacentViewAreas".to_string(), Value::Array(Vec::new())),
        ("uiAppearanceMode".to_string(), Value::Int(APPEARANCE_DARK)),
        (
            "uiAppearanceSetting".to_string(),
            Value::Int(APPEARANCE_AUTOMATIC),
        ),
        ("mapAppearanceMode".to_string(), Value::Int(APPEARANCE_DARK)),
        (
            "mapAppearanceSetting".to_string(),
            Value::Int(APPEARANCE_AUTOMATIC),
        ),
    ];
    if let Some(view) = area(d) {
        entry.push(("viewAreas".to_string(), Value::Array(vec![view])));
        entry.push(("initialViewArea".to_string(), Value::Int(0)));
    }
    if let Some(url) = d.initial_url.as_deref().filter(|u| !u.is_empty()) {
        entry.push(("initialURL".to_string(), Value::String(url.into())));
    }
    Value::Dict(entry)
}

fn hid_axis(v: i64) -> u16 {
    (v & 0xffff) as u16
}

pub fn display_uuids(cfg: &InfoConfig) -> (String, String) {
    match &cfg.theme_displays {
        Some(names) => (names.main.clone(), names.cluster.clone()),
        None => (MAIN_UUID.into(), ALT_UUID.into()),
    }
}

pub fn build(cfg: &InfoConfig) -> Value {
    let (main_uuid, alt_uuid) = display_uuids(cfg);
    let mut displays = vec![display(&cfg.main, STREAM_TYPE_MAIN_SCREEN, &main_uuid)];
    if let Some(cluster) = &cfg.cluster {
        displays.push(display(cluster, STREAM_TYPE_ALT_SCREEN, &alt_uuid));
    }
    let features = if cfg.disable_audio_output {
        CARPLAY_FEATURES & !CARPLAY_AUDIO_FEATURES
    } else {
        CARPLAY_FEATURES
    };

    let mut info = vec![
        (
            "sourceVersion".to_string(),
            Value::String(cfg.source_version.clone()),
        ),
        ("features".to_string(), Value::Int(features)),
        ("statusFlags".to_string(), Value::Int(4)),
        ("model".to_string(), Value::String("LIVI".into())),
        ("manufacturer".to_string(), Value::String("LIVI".into())),
        ("deviceID".to_string(), Value::String(cfg.device_id.clone())),
        (
            "bluetoothIDs".to_string(),
            Value::Array(vec![Value::String(cfg.bt_mac.clone())]),
        ),
        ("name".to_string(), Value::String(cfg.device_name.clone())),
        (
            "rightHandDrive".to_string(),
            Value::Bool(cfg.right_hand_drive),
        ),
        ("keepAliveLowPower".to_string(), Value::Bool(false)),
        ("keepAliveSendStatsAsBody".to_string(), Value::Bool(false)),
        ("modes".to_string(), modes()),
    ];
    if !cfg.disable_audio_output {
        info.push(("audioLatencies".to_string(), audio_latencies()));
        info.push((
            "audioFormats".to_string(),
            audio_formats(cfg.entertainment_sample_rate),
        ));
    }
    info.push((
        "extendedFeatures".to_string(),
        Value::Array(vec![
            Value::String("vocoderInfo".into()),
            Value::String("enhancedRequestCarUI".into()),
        ]),
    ));
    info.push(("displays".to_string(), Value::Array(displays)));
    let mut hid_devices = vec![
        hid::touch_device(
            hid_axis(cfg.main.width_pixels),
            hid_axis(cfg.main.height_pixels),
            &main_uuid,
        ),
        hid::knob_device(&main_uuid),
        hid::media_device(&main_uuid),
        hid::telephony_device(&main_uuid),
    ];
    if cfg.theme_displays.is_some() && cfg.cluster.is_some() {
        hid_devices.push(hid::cluster_knob_device(&alt_uuid));
    }
    info.push(("hidDevices".to_string(), Value::Array(hid_devices)));
    if !cfg.icons.is_empty() {
        info.push(("oemIconVisible".to_string(), Value::Bool(true)));
        info.push((
            "oemIconLabel".to_string(),
            Value::String(cfg.oem_label.clone()),
        ));
        let icons = cfg
            .icons
            .iter()
            .map(|icon| {
                dict([
                    ("imageData", Value::Data(icon.png.clone())),
                    ("widthPixels", Value::Int(icon.width_pixels)),
                    ("heightPixels", Value::Int(icon.height_pixels)),
                    ("prerendered", Value::Bool(true)),
                ])
            })
            .collect();
        info.push(("oemIcons".to_string(), Value::Array(icons)));
    }
    if cfg.hevc {
        info.push(("hevcInfo".to_string(), Value::Dict(Vec::new())));
    }
    Value::Dict(info)
}

pub fn add_display_panels(info: &mut Value, theme_displays: Option<&ThemeDisplays>) {
    let Some(displays) = info.get("displays").and_then(Value::as_array) else {
        return;
    };
    let size = |d: &Value, k: &str| d.get(k).cloned().unwrap_or(Value::Int(0));
    let panel = |d: &Value, uid: &str, streams: Vec<&str>, instruments: bool, plugin: bool| {
        let video = streams
            .iter()
            .map(|uuid| {
                dict([
                    ("uuid", Value::String((*uuid).into())),
                    ("originXPixels", Value::Int(0)),
                    ("originYPixels", Value::Int(0)),
                    ("zIndex", Value::Int(0)),
                ])
            })
            .collect();
        let mut fields = vec![
            ("uid".to_string(), Value::String(uid.to_string())),
            ("widthPixels".to_string(), size(d, "widthPixels")),
            ("heightPixels".to_string(), size(d, "heightPixels")),
            ("widthPhysical".to_string(), size(d, "widthPhysical")),
            ("heightPhysical".to_string(), size(d, "heightPhysical")),
            (
                "properties".to_string(),
                Value::Array(if instruments {
                    vec![Value::String("showsInstruments".into())]
                } else {
                    Vec::new()
                }),
            ),
            ("initialVideoStreams".to_string(), Value::Array(video)),
        ];
        if plugin {
            let composed = streams
                .iter()
                .map(|uuid| Value::String((*uuid).into()))
                .collect();
            let plugin_uid = if streams.is_empty() {
                format!("{CLUSTER_UID}-plugin")
            } else {
                uid.to_string()
            };
            let plugin = dict([
                ("uid", Value::String(plugin_uid)),
                ("videoStreams", Value::Array(composed)),
            ]);
            fields.push(("displayPlugins".to_string(), Value::Array(vec![plugin])));
        }
        Value::Dict(fields)
    };
    let is_alt = |d: &Value| d.get("type") == Some(&Value::Int(STREAM_TYPE_ALT_SCREEN));
    let mut panels: Vec<Value> = displays
        .iter()
        .filter_map(|d| {
            let uuid = d.get("uuid").and_then(Value::as_str)?;
            let uid = match theme_displays {
                Some(names) if is_alt(d) => names.cluster.clone(),
                Some(names) => names.main.clone(),
                None => format!("{uuid}-panel"),
            };
            let plugin = theme_displays.is_some() && is_alt(d);
            Some(panel(d, &uid, vec![uuid], is_alt(d), plugin))
        })
        .collect();
    let themed_cluster = theme_displays.is_some() && displays.iter().any(is_alt);
    if !themed_cluster {
        let Some(sized) = displays.iter().find(|d| is_alt(d)).or(displays.first()) else {
            return;
        };
        panels.push(panel(
            sized,
            &format!("{CLUSTER_UID}-panel"),
            Vec::new(),
            true,
            true,
        ));
    }
    if let Value::Dict(entries) = info {
        entries.push(("displayPanels".to_string(), Value::Array(panels)));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample() -> InfoConfig {
        InfoConfig {
            device_name: "LIVI".into(),
            device_id: "AA:BB:CC:DD:EE:FF".into(),
            bt_mac: "11:22:33:44:55:66".into(),
            source_version: "950.7.1".into(),
            hevc: true,
            main: DisplayConfig {
                width_pixels: 1280,
                height_pixels: 720,
                fps: Some(60),
                view_area: Some(Insets::default()),
                safe_area: Some(Insets {
                    top: 0,
                    bottom: 0,
                    left: 10,
                    right: 10,
                }),
                safe_area_draw_outside: Some(true),
                ..Default::default()
            },
            cluster: None,
            entertainment_sample_rate: 48000,
            disable_audio_output: false,
            oem_label: "App".into(),
            icons: Vec::new(),
            right_hand_drive: false,
            theme_displays: None,
        }
    }

    #[test]
    fn a_display_of_unknown_size_grows_with_its_pixels() {
        let size = |cfg: &InfoConfig| {
            let info = build(cfg);
            let main = &info.get("displays").and_then(Value::as_array).unwrap()[0];
            assert_eq!(main.get("type"), Some(&Value::Int(110)));
            for key in ["uiAppearanceMode", "mapAppearanceMode"] {
                assert_eq!(main.get(key), Some(&Value::Int(APPEARANCE_DARK)), "{key}");
            }
            assert_eq!(
                main.get("uiAppearanceSetting"),
                Some(&Value::Int(APPEARANCE_AUTOMATIC))
            );
            (
                main.get("widthPhysical").cloned(),
                main.get("heightPhysical").cloned(),
            )
        };
        assert_eq!(
            size(&sample()),
            (Some(Value::Int(163)), Some(Value::Int(92)))
        );
        let mut wide = sample();
        wide.main.width_pixels = 1920;
        wide.main.height_pixels = 1080;
        assert_eq!(size(&wide), (Some(Value::Int(244)), Some(Value::Int(137))));
    }

    #[test]
    fn mm_at_dpi_never_drops_to_zero() {
        assert_eq!(mm_at_dpi(1920, 160.0), 305);
        assert_eq!(mm_at_dpi(0, 160.0), 1);
    }

    #[test]
    fn no_audio_drops_formats_and_audio_features() {
        let mut cfg = sample();
        cfg.disable_audio_output = true;
        let info = build(&cfg);
        assert!(info.get("audioFormats").is_none());
        assert_eq!(
            info.get("features"),
            Some(&Value::Int(CARPLAY_FEATURES & !CARPLAY_AUDIO_FEATURES))
        );
    }

    #[test]
    fn main_panel_stays_without_plugin_and_a_virtual_cluster_carries_one() {
        let mut info = build(&sample());
        assert!(info.get("displayPanels").is_none());
        add_display_panels(&mut info, None);
        let panels = info.get("displayPanels").and_then(Value::as_array).unwrap();
        assert_eq!(panels.len(), 2);
        let main = &panels[0];
        assert_eq!(
            main.get("uid").and_then(Value::as_str),
            Some(format!("{MAIN_UUID}-panel").as_str())
        );
        assert_eq!(main.get("widthPixels"), Some(&Value::Int(1280)));
        assert_eq!(main.get("heightPhysical"), Some(&Value::Int(92)));
        assert_eq!(main.get("properties"), Some(&Value::Array(Vec::new())));
        assert!(main.get("displayPlugins").is_none());
        let stream = &main
            .get("initialVideoStreams")
            .and_then(Value::as_array)
            .unwrap()[0];
        assert_eq!(stream.get("uuid").and_then(Value::as_str), Some(MAIN_UUID));

        let cluster = &panels[1];
        assert_eq!(
            cluster.get("uid").and_then(Value::as_str),
            Some(format!("{CLUSTER_UID}-panel").as_str())
        );
        assert_eq!(cluster.get("widthPixels"), Some(&Value::Int(1280)));
        assert_eq!(
            cluster.get("properties"),
            Some(&Value::Array(vec![Value::String(
                "showsInstruments".into()
            )]))
        );
        assert_eq!(
            cluster.get("initialVideoStreams"),
            Some(&Value::Array(Vec::new()))
        );
        let plugins = cluster
            .get("displayPlugins")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(
            plugins[0].get("uid").and_then(Value::as_str),
            Some(format!("{CLUSTER_UID}-plugin").as_str())
        );
        assert_eq!(
            plugins[0].get("videoStreams"),
            Some(&Value::Array(Vec::new()))
        );
    }

    #[test]
    fn an_alternate_screen_keeps_its_stream_and_the_plugin_gets_its_own_panel() {
        let mut cfg = sample();
        let mut cluster = cfg.main.clone();
        cluster.width_pixels = 800;
        cfg.cluster = Some(cluster);
        let mut info = build(&cfg);
        add_display_panels(&mut info, None);
        let panels = info.get("displayPanels").and_then(Value::as_array).unwrap();
        assert_eq!(panels.len(), 3);
        assert!(panels[0].get("displayPlugins").is_none());

        let alt = &panels[1];
        assert_eq!(
            alt.get("uid").and_then(Value::as_str),
            Some(format!("{ALT_UUID}-panel").as_str())
        );
        assert!(
            alt.get("displayPlugins").is_none(),
            "a plug-in would black out the cluster"
        );
        assert_eq!(
            alt.get("properties"),
            Some(&Value::Array(vec![Value::String(
                "showsInstruments".into()
            )]))
        );
        let stream = &alt
            .get("initialVideoStreams")
            .and_then(Value::as_array)
            .unwrap()[0];
        assert_eq!(stream.get("uuid").and_then(Value::as_str), Some(ALT_UUID));

        let virtual_panel = &panels[2];
        assert_eq!(
            virtual_panel.get("uid").and_then(Value::as_str),
            Some(format!("{CLUSTER_UID}-panel").as_str())
        );
        assert_eq!(virtual_panel.get("widthPixels"), Some(&Value::Int(800)));
        assert_eq!(
            virtual_panel.get("initialVideoStreams"),
            Some(&Value::Array(Vec::new()))
        );
        let plugins = virtual_panel
            .get("displayPlugins")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(
            plugins[0].get("videoStreams"),
            Some(&Value::Array(Vec::new()))
        );
    }

    fn names() -> ThemeDisplays {
        ThemeDisplays {
            main: "Center_Display".into(),
            cluster: "Cluster_Display".into(),
        }
    }

    #[test]
    fn a_theme_names_the_displays_too() {
        let mut cfg = sample();
        cfg.cluster = Some(cfg.main.clone());
        cfg.theme_displays = Some(names());
        let info = build(&cfg);
        let displays = info.get("displays").and_then(Value::as_array).unwrap();
        let uuid = |d: &Value| d.get("uuid").and_then(Value::as_str).map(str::to_string);
        assert_eq!(uuid(&displays[0]).as_deref(), Some("Center_Display"));
        assert_eq!(uuid(&displays[1]).as_deref(), Some("Cluster_Display"));
        let hid = info.get("hidDevices").and_then(Value::as_array).unwrap();
        let knob = hid
            .iter()
            .find(|d| d.get("name").and_then(Value::as_str) == Some("LIVI Cluster Knob"));
        assert_eq!(
            knob.and_then(|k| k.get("displayUUID"))
                .and_then(Value::as_str),
            Some("Cluster_Display")
        );
        let plain = build(&sample());
        assert_eq!(
            plain
                .get("hidDevices")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(4)
        );
        assert_eq!(
            display_uuids(&sample()),
            (MAIN_UUID.to_string(), ALT_UUID.to_string())
        );
    }

    #[test]
    fn a_theme_names_the_panels_after_its_displays() {
        let mut cfg = sample();
        cfg.cluster = Some(cfg.main.clone());
        let mut info = build(&cfg);
        add_display_panels(&mut info, Some(&names()));
        let panels = info.get("displayPanels").and_then(Value::as_array).unwrap();
        let uid = |p: &Value| p.get("uid").and_then(Value::as_str).map(str::to_string);
        assert_eq!(
            panels.len(),
            2,
            "the cluster carries the plug-in, no extra panel"
        );
        assert_eq!(uid(&panels[0]).as_deref(), Some("Center_Display"));
        assert!(panels[0].get("displayPlugins").is_none());
        assert_eq!(uid(&panels[1]).as_deref(), Some("Cluster_Display"));
        let plugin = &panels[1]
            .get("displayPlugins")
            .and_then(Value::as_array)
            .unwrap()[0];
        assert_eq!(uid(plugin).as_deref(), Some("Cluster_Display"));
        assert_eq!(
            plugin.get("videoStreams"),
            Some(&Value::Array(vec![Value::String(ALT_UUID.into())]))
        );
    }

    #[test]
    fn display_panels_need_displays() {
        let mut info = Value::Array(Vec::new());
        add_display_panels(&mut info, None);
        assert_eq!(info, Value::Array(Vec::new()));
        let mut info = dict([("displays", Value::Array(Vec::new()))]);
        let before = info.clone();
        add_display_panels(&mut info, None);
        assert_eq!(info, before);
    }

    #[test]
    fn a_display_without_uuid_or_sizes_still_yields_a_cluster() {
        let mut info = dict([(
            "displays",
            Value::Array(vec![dict([("type", Value::Int(110))])]),
        )]);
        add_display_panels(&mut info, None);
        let panels = info.get("displayPanels").and_then(Value::as_array).unwrap();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].get("widthPixels"), Some(&Value::Int(0)));
        assert!(panels[0].get("displayPlugins").is_some());
    }

    #[test]
    fn the_status_bar_edge_goes_out_only_when_set() {
        let edge = |info: &Value| {
            let main = &info.get("displays").and_then(Value::as_array).unwrap()[0];
            let area = &main.get("viewAreas").and_then(Value::as_array).unwrap()[0];
            area.get("viewAreaStatusBarEdge").cloned()
        };
        assert_eq!(edge(&build(&sample())), None);
        let mut cfg = sample();
        cfg.main.status_bar_edge = Some(1);
        assert_eq!(edge(&build(&cfg)), Some(Value::Int(1)));
    }

    #[test]
    fn speech_mode_minus_one_goes_out_as_a_real() {
        let info = build(&sample());
        let states = info
            .get("modes")
            .and_then(|m| m.get("appStates"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(states[1].get("speechMode"), Some(&Value::Real(-1.0)));
    }
}
