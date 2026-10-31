// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::message::Action;
use serde_json::Value;

use crate::i18n::Strings;
use crate::icons::settings_icon;
use crate::settings::calibration;
use crate::settings::custom::{self, rows as custom_rows};
use crate::settings::node::{
    Choice, CustomPage, Kind, Label, Node, OnPick, OptionValue, Radio, Shown,
};
use crate::settings::sources::{Offer, holds, is_dongle, offers, unit};
use crate::settings::value::{as_f64, get, patch, same};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowKind {
    Route,
    #[default]
    Value,
    Switch,
    Slider,
    KeyBinding,
    Custom,
    Devices,
    Info,
    Button,
    Progress,
    Item,
    Device,
    Picture,
    Pick,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    pub initials: String,
    pub color: [u8; 3],
    pub logo: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub icon: &'static str,
    pub label: String,
    pub on: bool,
    pub tint: Option<[u8; 3]>,
    pub enabled: bool,
    pub action: Action,
    pub switch: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub kind: RowKind,
    pub node: usize,
    pub label: String,
    pub icon: Option<&'static str>,
    pub value: String,
    pub checked: bool,
    pub level: f32,
    pub enabled: bool,
    pub swatch: Option<String>,
    pub custom: Option<CustomPage>,
    pub button: String,
    pub outlined: bool,
    pub action: Option<Action>,
    pub badge: Option<Badge>,
    pub dimmed: bool,
    pub tools: Vec<Tool>,
    pub change: Option<Change>,
    pub local: Option<Local>,
    pub mono: bool,
    pub alert: bool,
    pub device: Option<Device>,
    pub binding: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Active,
    Available,
    Offline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub protocol_icon: &'static str,
    pub transport_icon: Option<&'static str>,
    pub battery: Option<(u8, bool)>,
    pub signal: Option<u8>,
    pub presence: Presence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Local {
    CloseDialog,
    ToProjection,
    Browse(crate::settings::custom::files::Wanted),
    PickEntry(usize),
    CalibrateTouch(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub title: String,
    pub rows: Vec<Row>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    pub name: String,
    pub homepage: String,
    pub source: String,
    pub copyright: String,
    pub license: String,
    pub version: String,
    pub sha: String,
    pub run: String,
}

impl Row {
    pub fn focusable(&self) -> bool {
        !matches!(self.kind, RowKind::Info | RowKind::Progress)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub label: String,
    pub selected: bool,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    List {
        title: String,
        rows: Vec<Row>,
        dialog: Option<Dialog>,
    },
    Select {
        title: String,
        options: Vec<Pick>,
        read_only: bool,
    },
    Number {
        title: String,
        value: f64,
        min: Option<f64>,
        max: Option<f64>,
        step: f64,
        text: String,
    },
    Text {
        title: String,
        value: String,
        min_len: Option<usize>,
        max_len: Option<usize>,
        url: bool,
    },
    Color {
        title: String,
        value: String,
        custom: bool,
    },
    Slider {
        title: String,
        level: f32,
        text: String,
    },
    Calibration {
        title: String,
        pattern: calibration::Pattern,
        knobs: Vec<calibration::Knob>,
    },
    Custom {
        title: String,
        page: CustomPage,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Config(Value),
    DongleOn(Radio),
    Act(Action),
}

pub fn label(t: &Strings, l: &Label) -> String {
    if let Some((script, latin)) = l.unreadable
        && !t.shows(script)
    {
        return latin.to_string();
    }
    match l.key {
        Some(key) => t.t(key, l.text),
        None => l.text.to_string(),
    }
}

fn calibration_page(node: &Node) -> Option<CustomPage> {
    let page = match &node.kind {
        Kind::Custom(page) => *page,
        Kind::Route { children, .. } if children.len() == 1 => match children[0].kind {
            Kind::Custom(page) => page,
            _ => return None,
        },
        _ => return None,
    };
    calibration::page(page, &Value::Null).map(|_| page)
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

fn option_value(v: &OptionValue) -> Value {
    match v {
        OptionValue::Text(s) => Value::from(*s),
        OptionValue::Number(n) => number_value(*n),
    }
}

fn offline_label(t: &Strings, o: &Offer) -> String {
    if o.offline {
        t.with(
            "settings.audioDeviceOffline",
            "{{name}} (offline)",
            &[("name", &o.label)],
        )
    } else {
        o.label.clone()
    }
}

pub fn default_color(path: &str) -> &'static str {
    match path {
        "primaryColorDark" => "#00adad",
        "primaryColorLight" => "#008585",
        "highlightColorLight" => "#007575",
        "backgroundColorDark" => "#000000",
        "backgroundColorLight" => "#f5f5f5",
        "textColorDark" => "#ffffff",
        "textColorLight" => "#000000",
        _ => "#009494",
    }
}

fn choices(node: &Node, state: &Value, t: &Strings) -> Vec<Offer> {
    let Kind::Select {
        options, source, ..
    } = &node.kind
    else {
        return Vec::new();
    };
    let fixed = options.iter().map(|c: &Choice| Offer {
        label: label(t, &c.label),
        value: option_value(&c.value),
        offline: false,
    });
    let mut out: Vec<Offer> = fixed.collect();
    if let Some(src) = source {
        for o in offers(*src, state, t) {
            if !out.iter().any(|x| same(&x.value, &o.value)) {
                out.push(o);
            }
        }
    }
    out
}

pub fn number_text(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub fn typed_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn shown_text(shown: Shown, v: f64) -> String {
    match shown {
        Shown::Plain => number_text(v),
        Shown::Percent => format!("{} %", (v * 100.0).round() as i64),
        Shown::Suffix(s) => format!("{}{s}", number_text(v)),
        Shown::DashIfNegative => {
            if v < 0.0 {
                "-".into()
            } else {
                number_text(v)
            }
        }
    }
}

fn display(node: &Node, state: &Value, t: &Strings) -> String {
    let config = &state["config"];
    let raw = get(config, node.path);
    match &node.kind {
        Kind::Select { label_path, .. } => choices(node, state, t)
            .into_iter()
            .find(|o| same(&o.value, raw))
            .map(|o| offline_label(t, &o))
            .or_else(|| label_path.and_then(|p| get(config, p).as_str().map(str::to_string)))
            .unwrap_or_else(|| raw.as_str().map(str::to_string).unwrap_or_default()),
        Kind::Number { unit: Some(u), .. } => {
            let u = unit(*u, config);
            let step = u.step.unwrap_or(1.0);
            as_f64(raw).map_or_else(String::new, |v| {
                let shown =
                    ((v * u.factor / step).round() * step).min(u.max.unwrap_or(f64::INFINITY));
                format!("{} {}", number_text(shown), u.suffix)
            })
        }
        Kind::Number { default, .. } => as_f64(raw)
            .or(*default)
            .map_or_else(String::new, |v| shown_text(node.shown, v)),
        Kind::Slider => as_f64(raw).map_or_else(String::new, |v| shown_text(node.shown, v)),
        Kind::Text { .. } | Kind::Color | Kind::KeyBinding { .. } => {
            raw.as_str().unwrap_or("").to_string()
        }
        _ => String::new(),
    }
}

fn row(node: &Node, state: &Value, t: &Strings) -> Row {
    let config = &state["config"];
    if let Kind::KeyBinding { binding } = node.kind {
        return custom::binding_row(
            label(t, &node.label),
            node.icon.and_then(settings_icon),
            binding,
            config,
        );
    }
    let raw = get(config, node.path);
    let mut enabled = node.disabled.is_none_or(|c| !holds(c, &state["system"]));
    if let Kind::Select {
        read_only: Some(c), ..
    } = &node.kind
    {
        enabled &= !holds(*c, &state["system"]);
    }
    let kind = match &node.kind {
        Kind::Route { .. } => RowKind::Route,
        Kind::Checkbox | Kind::Toggle => RowKind::Switch,
        Kind::Slider if node.page_title.is_none() => RowKind::Slider,
        Kind::KeyBinding { .. } => RowKind::KeyBinding,
        Kind::Custom(_) => RowKind::Custom,
        Kind::BtDeviceList => RowKind::Devices,
        _ => RowKind::Value,
    };
    Row {
        kind,
        label: label(t, &node.label),
        icon: node.icon.and_then(settings_icon),
        value: if node.display_value || kind == RowKind::KeyBinding {
            display(node, state, t)
        } else {
            String::new()
        },
        checked: raw.as_bool().unwrap_or(false),
        level: as_f64(raw).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
        enabled,
        swatch: matches!(node.kind, Kind::Color).then(|| {
            raw.as_str()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(default_color(node.path))
                .to_string()
        }),
        custom: match node.kind {
            Kind::Custom(p) => Some(p),
            _ => None,
        },
        ..Row::default()
    }
}

pub struct Settings {
    pub facts: Facts,
    root: Node,
}

impl Settings {
    pub fn new(root: Node) -> Self {
        Self {
            root,
            facts: Facts::default(),
        }
    }

    pub fn with_facts(mut self, facts: Facts) -> Self {
        self.facts = facts;
        self
    }

    pub fn watches(&self, stack: &[usize]) -> Vec<custom::Watch> {
        self.node(stack)
            .map(|n| {
                n.children()
                    .iter()
                    .filter_map(|c| match c.kind {
                        Kind::Custom(page) => custom::watch(page),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn node(&self, stack: &[usize]) -> Option<&Node> {
        stack
            .iter()
            .try_fold(&self.root, |n, &i| n.children().get(i))
    }

    pub fn screen(&self, stack: &[usize], state: &Value, t: &Strings) -> Option<Screen> {
        let node = self.node(stack)?;
        let title = label(t, node.page_title.as_ref().unwrap_or(&node.label));
        let config = &state["config"];
        let raw = get(config, node.path);
        if let Some((pattern, knobs)) =
            calibration_page(node).and_then(|p| calibration::page(p, state))
        {
            return Some(Screen::Calibration {
                title,
                pattern,
                knobs,
            });
        }
        Some(match &node.kind {
            Kind::Route { children, .. } => Screen::List {
                title,
                rows: children
                    .iter()
                    .enumerate()
                    .flat_map(|(i, c)| {
                        let inline = match c.kind {
                            Kind::Custom(page) => custom_rows(page, state, t, &self.facts),
                            Kind::BtDeviceList => Some(custom::devices(state, t)),
                            _ => None,
                        };
                        inline
                            .unwrap_or_else(|| vec![row(c, state, t)])
                            .into_iter()
                            .map(move |r| Row { node: i, ..r })
                    })
                    .collect(),
                dialog: children.iter().find_map(|c| match c.kind {
                    Kind::Custom(page) => custom::dialog(page, state, t, &self.facts),
                    _ => None,
                }),
            },
            Kind::Select { read_only, .. } => Screen::Select {
                title,
                options: choices(node, state, t)
                    .into_iter()
                    .map(|o| Pick {
                        selected: same(&o.value, raw),
                        label: offline_label(t, &o),
                        offline: o.offline,
                    })
                    .collect(),
                read_only: read_only.is_some_and(|c| holds(c, &state["system"])),
            },
            Kind::Number {
                min,
                max,
                step,
                default,
                unit: u,
            } => {
                let u = u.map(|u| unit(u, config));
                let factor = u.map_or(1.0, |u| u.factor);
                let value = as_f64(raw).or(*default).unwrap_or(0.0) * factor;
                Screen::Number {
                    text: display(node, state, t),
                    title,
                    value,
                    min: min.map(|m| m * factor),
                    max: u.and_then(|u| u.max).or(max.map(|m| m * factor)),
                    step: u.and_then(|u| u.step).or(*step).unwrap_or(1.0),
                }
            }
            Kind::Text {
                min_len,
                max_len,
                url,
            } => Screen::Text {
                title,
                value: raw.as_str().unwrap_or("").to_string(),
                min_len: *min_len,
                max_len: *max_len,
                url: *url,
            },
            Kind::Color => {
                let set = raw.as_str().filter(|s| !s.trim().is_empty());
                Screen::Color {
                    title,
                    value: set.unwrap_or(default_color(node.path)).to_string(),
                    custom: set.is_some(),
                }
            }
            Kind::Slider => Screen::Slider {
                title,
                level: as_f64(raw).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
                text: display(node, state, t),
            },
            Kind::Custom(page) => Screen::Custom { title, page: *page },
            _ => return None,
        })
    }

    pub fn find(&self, keys: &[&str]) -> Option<Vec<usize>> {
        let mut stack = Vec::new();
        for key in keys {
            let i = self
                .node(&stack)?
                .children()
                .iter()
                .position(|c| c.route_key() == Some(*key) || c.path == *key)?;
            stack.push(i);
        }
        Some(stack)
    }

    pub fn opens(&self, stack: &[usize], index: usize) -> bool {
        self.node(stack)
            .and_then(|n| n.children().get(index))
            .is_some_and(|c| match &c.kind {
                Kind::Route { .. } => true,
                Kind::Checkbox | Kind::Toggle | Kind::KeyBinding { .. } | Kind::BtDeviceList => {
                    false
                }
                Kind::Slider => c.page_title.is_some(),
                _ => true,
            })
    }

    pub fn calibration_key(&self, stack: &[usize], index: usize) -> Option<&'static str> {
        let page = calibration_page(self.node(stack)?)?;
        let (_, knobs) = calibration::page(page, &Value::Null)?;
        knobs.get(index).map(|k| k.key)
    }

    pub fn toggle(&self, stack: &[usize], index: usize, state: &Value) -> Option<Change> {
        let node = self.node(stack)?.children().get(index)?;
        matches!(node.kind, Kind::Checkbox | Kind::Toggle).then(|| {
            let now = get(&state["config"], node.path).as_bool().unwrap_or(false);
            Change::Config(patch(node.path, Value::from(!now)))
        })
    }

    pub fn level(&self, stack: &[usize], index: Option<usize>, level: f32) -> Option<Change> {
        let node = match index {
            Some(i) => self.node(stack)?.children().get(i)?,
            None => self.node(stack)?,
        };
        matches!(node.kind, Kind::Slider).then(|| {
            let v = (f64::from(level.clamp(0.0, 1.0)) * 100.0).round() / 100.0;
            Change::Config(patch(node.path, Value::from(v)))
        })
    }

    pub fn pick(&self, stack: &[usize], option: usize, state: &Value, t: &Strings) -> Vec<Change> {
        let Some(node) = self.node(stack) else {
            return Vec::new();
        };
        let Kind::Select {
            label_path,
            on_pick,
            read_only,
            ..
        } = &node.kind
        else {
            return Vec::new();
        };
        if read_only.is_some_and(|c| holds(c, &state["system"])) {
            return Vec::new();
        }
        let Some(o) = choices(node, state, t).into_iter().nth(option) else {
            return Vec::new();
        };
        let mut out = vec![Change::Config(patch(node.path, o.value.clone()))];
        if let Some(p) = label_path {
            out.push(Change::Config(patch(p, Value::from(o.label))));
        }
        if let Some(OnPick::SwitchDongleOn(radio)) = on_pick
            && is_dongle(&o.value)
        {
            out.push(Change::DongleOn(*radio));
        }
        out
    }

    pub fn number(&self, stack: &[usize], shown: f64, state: &Value) -> Option<Change> {
        let node = self.node(stack)?;
        let Kind::Number {
            min, max, unit: u, ..
        } = &node.kind
        else {
            return None;
        };
        let factor = u.map_or(1.0, |u| unit(u, &state["config"]).factor);
        let mut stored = shown / factor;
        if u.is_some() {
            stored = stored.round();
        }
        if let Some(m) = min {
            stored = stored.max(*m);
        }
        if let Some(m) = max {
            stored = stored.min(*m);
        }
        Some(Change::Config(patch(node.path, number_value(stored))))
    }

    pub fn color_reset(&self, stack: &[usize]) -> Option<Change> {
        let node = self.node(stack)?;
        matches!(node.kind, Kind::Color).then(|| Change::Config(patch(node.path, Value::Null)))
    }

    pub fn text(&self, stack: &[usize], text: &str) -> Option<Change> {
        let node = self.node(stack)?;
        match &node.kind {
            Kind::Text {
                min_len,
                max_len,
                url,
            } => {
                let v = if *url { text.trim() } else { text };
                let n = v.chars().count();
                let ok = min_len.is_none_or(|m| n >= m)
                    && max_len.is_none_or(|m| n <= m)
                    && (!*url || v.starts_with("http://") || v.starts_with("https://"));
                ok.then(|| Change::Config(patch(node.path, Value::from(v))))
            }
            Kind::Color => {
                let hex = text.trim();
                let hex = hex.strip_prefix('#').unwrap_or(hex);
                let ok = hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit());
                ok.then(|| {
                    Change::Config(patch(
                        node.path,
                        Value::from(format!("#{}", hex.to_lowercase())),
                    ))
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings::schema;

    #[test]
    fn whole_numbers_go_out_as_integers() {
        assert_eq!(option_value(&OptionValue::Number(90.0)), json!(90));
        assert!(option_value(&OptionValue::Number(90.0)).is_u64());
        assert_eq!(option_value(&OptionValue::Number(0.5)), json!(0.5));
        assert_eq!(option_value(&OptionValue::Number(-3.0)), json!(-3));
    }

    #[test]
    fn typed_numbers_take_a_comma_or_a_point() {
        assert_eq!(typed_number(" 1080 "), Some(1080.0));
        assert_eq!(typed_number("2,5"), Some(2.5));
        assert_eq!(typed_number("2.5"), Some(2.5));
        assert_eq!(typed_number("abc"), None);
        assert_eq!(typed_number("inf"), None);
        assert_eq!(number_text(1080.0), "1080");
    }

    #[test]
    fn a_whole_number_entry_is_saved_as_an_integer() {
        let s = Settings::new(schema());
        let Some(stack) = s.find(&["general", "uiZoomPercent"]) else {
            panic!("ui zoom is in general");
        };
        let Some(Change::Config(patch)) = s.number(&stack, 110.0, &state()) else {
            panic!("ui zoom is a number");
        };
        assert!(patch["uiZoomPercent"].is_u64());
        assert_eq!(patch["uiZoomPercent"], json!(110));
    }

    #[test]
    fn a_hex_colour_is_taken_with_or_without_the_hash() {
        let s = Settings::new(schema());
        let Some(stack) = s.find(&["appearance", "ui-colors", "highlightColorLight"]) else {
            panic!("highlight colour is in appearance");
        };
        let saved = |text: &str| match s.text(&stack, text) {
            Some(Change::Config(patch)) => {
                patch["highlightColorLight"].as_str().map(str::to_string)
            }
            _ => None,
        };
        assert_eq!(saved("#ff7e00").as_deref(), Some("#ff7e00"));
        assert_eq!(saved(" FF7E00 ").as_deref(), Some("#ff7e00"));
        assert_eq!(saved("#ff7e0"), None);
        assert_eq!(saved("#ff7e00a"), None);
        assert_eq!(saved("gg7e00"), None);
    }

    fn state() -> Value {
        json!({
            "config": {
                "huVolume": 0.42,
                "darkMode": true,
                "audioOutputDevice": "",
                "samplingFrequency": 1
            },
            "system": { "audioSinks": [{ "id": "hdmi", "name": "HDMI", "offline": false }] }
        })
    }

    fn audio(s: &Settings) -> Vec<usize> {
        let i = s
            .node(&[])
            .unwrap()
            .children()
            .iter()
            .position(|c| c.route_key() == Some("audio"))
            .unwrap();
        vec![i]
    }

    #[test]
    fn the_root_lists_the_sections() {
        let s = Settings::new(schema());
        let Some(Screen::List { rows, .. }) = s.screen(&[], &state(), &Strings::default()) else {
            panic!("root is a list");
        };
        assert!(rows.iter().all(|r| r.kind == RowKind::Route));
        assert!(
            rows.iter()
                .any(|r| r.label == "Audio" && r.icon == Some("VolumeUpOutlined"))
        );
    }

    #[test]
    fn audio_rows_show_values() {
        let s = Settings::new(schema());
        let Some(Screen::List { rows, .. }) = s.screen(&audio(&s), &state(), &Strings::default())
        else {
            panic!("audio is a list");
        };
        assert_eq!(rows[0].value, "42 %");
        assert_eq!(rows[0].kind, RowKind::Value);
        let freq = rows
            .iter()
            .find(|r| r.label == "Sampling Frequency")
            .unwrap();
        assert_eq!(freq.value, "48 kHz");
        let out = rows.iter().find(|r| r.label == "Audio Output").unwrap();
        assert_eq!(out.value, "System default");
    }

    #[test]
    fn picking_writes_value_and_label() {
        let s = Settings::new(schema());
        let mut stack = audio(&s);
        let out = s
            .node(&stack)
            .unwrap()
            .children()
            .iter()
            .position(|c| c.path == "audioOutputDevice")
            .unwrap();
        stack.push(out);
        let changes = s.pick(&stack, 1, &state(), &Strings::default());
        assert_eq!(
            changes,
            vec![
                Change::Config(json!({ "audioOutputDevice": "hdmi" })),
                Change::Config(json!({ "audioOutputDeviceLabel": "HDMI" })),
            ]
        );
    }

    #[test]
    fn switches_flip() {
        let s = Settings::new(schema());
        let i = s
            .node(&[])
            .unwrap()
            .children()
            .iter()
            .position(|c| c.route_key() == Some("appearance"))
            .unwrap();
        let dark = s
            .node(&[i])
            .unwrap()
            .children()
            .iter()
            .position(|c| c.path == "darkMode")
            .unwrap();
        assert_eq!(
            s.toggle(&[i], dark, &state()),
            Some(Change::Config(json!({ "darkMode": false })))
        );
    }
}
