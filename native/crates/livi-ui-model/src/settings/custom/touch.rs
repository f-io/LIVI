// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::TouchCalibration;
use livi_core_proto::message::Action;
use serde_json::{Value, json};

use super::info;
use crate::i18n::Strings;
use crate::settings::node::Screen;
use crate::settings::sources::display_for;
use crate::settings::view::{Change, Local, Row, RowKind, Tool};

pub const TARGETS: [(f64, f64); 4] = [(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)];

pub fn entry(state: &Value, name: &str) -> TouchCalibration {
    serde_json::from_value(state["config"]["touchCalibration"][name].clone()).unwrap_or_default()
}

pub fn store(name: &str, entry: &TouchCalibration) -> Value {
    json!({ "touchCalibration": { name: entry } })
}

fn set(name: &str, entry: &TouchCalibration) -> Action {
    Action::SetConfig {
        patch: store(name, entry),
    }
}

fn names(display: &Value) -> Vec<&str> {
    display["touch"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

pub fn on_main(state: &Value, n: usize) -> Option<(String, String)> {
    let display = display_for(state, Screen::Main)?;
    let name = names(display).get(n)?.to_string();
    let id = display["id"].as_str().unwrap_or_default().to_string();
    Some((name, id))
}

pub fn solve(touched: &[(f64, f64)]) -> Option<[f64; 6]> {
    if touched.len() != TARGETS.len() {
        return None;
    }
    let mut ata = [[0.0f64; 3]; 3];
    let (mut atx, mut aty) = ([0.0f64; 3], [0.0f64; 3]);
    for (&(u, v), &(tx, ty)) in touched.iter().zip(&TARGETS) {
        let row = [u, v, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atx[i] += row[i] * tx;
            aty[i] += row[i] * ty;
        }
    }
    let x = solve3(ata, atx)?;
    let y = solve3(ata, aty)?;
    Some([x[0], x[1], x[2], y[0], y[1], y[2]])
}

fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(&m);
    if d.abs() < 1e-9 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, v) in out.iter_mut().enumerate() {
        let mut mk = m;
        for (row, value) in mk.iter_mut().zip(b) {
            row[k] = value;
        }
        *v = det(&mk) / d;
    }
    Some(out)
}

fn device(screen: Screen, n: usize, name: &str, id: &str, state: &Value, t: &Strings) -> Vec<Row> {
    let e = entry(state, name);
    let toggled = TouchCalibration {
        display: id.to_string(),
        enabled: !e.enabled,
        ..e.clone()
    };
    let mut out = vec![Row {
        kind: RowKind::Switch,
        label: t.t("settings.touchCalibration", "Calibration"),
        checked: e.enabled,
        enabled: true,
        change: Some(Change::Config(store(name, &toggled))),
        ..Row::default()
    }];
    if screen == Screen::Main {
        out.push(Row {
            kind: RowKind::Button,
            label: name.to_string(),
            button: t.t("settings.touchCalibrate", "Calibrate"),
            outlined: true,
            enabled: true,
            local: Some(Local::CalibrateTouch(n)),
            ..Row::default()
        });
    }
    out.push(Row {
        kind: RowKind::Button,
        label: t.t("settings.touchReset", "Reset Calibration"),
        button: t.t("settings.reset", "Reset"),
        outlined: true,
        enabled: state["config"]["touchCalibration"][name].is_object(),
        action: Some(Action::SetConfig {
            patch: json!({ "touchCalibration": { name: null } }),
        }),
        ..Row::default()
    });
    out
}

pub fn rows(screen: Screen, state: &Value, t: &Strings) -> Vec<Row> {
    let Some(display) = display_for(state, screen) else {
        return vec![info(
            t.t("settings.touchNone", "No touchscreen"),
            None,
            String::new(),
        )];
    };
    let id = display["id"].as_str().unwrap_or_default();
    let mut out: Vec<Row> = names(display)
        .iter()
        .enumerate()
        .flat_map(|(n, name)| device(screen, n, name, id, state, t))
        .collect();
    let elsewhere = state["system"]["displays"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["id"].as_str() != Some(id))
        .flat_map(names);
    for name in elsewhere {
        let moved = TouchCalibration {
            display: id.to_string(),
            ..entry(state, name)
        };
        out.push(Row {
            kind: RowKind::Item,
            label: name.to_string(),
            enabled: true,
            dimmed: true,
            tools: vec![Tool {
                icon: "LinkOutlined",
                label: t.t("settings.touchUseHere", "Use here"),
                on: false,
                tint: None,
                enabled: true,
                switch: false,
                action: set(name, &moved),
            }],
            ..Row::default()
        });
    }
    if out.is_empty() {
        out.push(info(
            t.t("settings.touchNone", "No touchscreen"),
            None,
            String::new(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({
            "config": {
                "touchCalibration": {
                    "ILITEK ILITEK-TP": {
                        "display": "DEL-a0b1-00000001",
                        "enabled": true,
                        "matrix": [1.0, 0.0, 0.02, 0.0, 1.0, 0.0]
                    }
                }
            },
            "system": {
                "displays": [
                    {
                        "connector": "HDMI-A-1",
                        "id": "DEL-a0b1-00000001",
                        "screen": "main",
                        "touch": ["ILITEK ILITEK-TP"]
                    },
                    {
                        "connector": "DSI-1",
                        "id": "DSI-1",
                        "screen": "cluster",
                        "touch": ["Goodix Capacitive TouchScreen"]
                    }
                ]
            }
        })
    }

    #[test]
    fn the_main_screen_offers_the_switch_and_the_assistant() {
        let t = Strings::new("en");
        let rows = rows(Screen::Main, &state(), &t);
        assert_eq!(rows[0].label, "Calibration");
        assert!(rows[0].checked);
        let Some(Change::Config(off)) = &rows[0].change else {
            panic!()
        };
        assert_eq!(
            off["touchCalibration"]["ILITEK ILITEK-TP"]["enabled"],
            false
        );
        assert_eq!(
            off["touchCalibration"]["ILITEK ILITEK-TP"]["matrix"],
            json!([1.0, 0.0, 0.02, 0.0, 1.0, 0.0])
        );
        assert_eq!(rows[1].label, "ILITEK ILITEK-TP");
        assert_eq!(rows[1].local, Some(Local::CalibrateTouch(0)));
        assert!(rows[2].enabled);
        assert_eq!(
            rows[2].action,
            Some(Action::SetConfig {
                patch: json!({ "touchCalibration": { "ILITEK ILITEK-TP": null } })
            })
        );
        assert_eq!(
            on_main(&state(), 0),
            Some(("ILITEK ILITEK-TP".into(), "DEL-a0b1-00000001".into()))
        );
    }

    #[test]
    fn switching_on_links_the_device_to_this_display() {
        let t = Strings::new("en");
        let rows = rows(Screen::Cluster, &state(), &t);
        assert!(!rows[0].checked);
        assert_eq!(rows[1].label, "Reset Calibration");
        assert!(!rows[1].enabled, "nothing to reset yet");
        assert_eq!(rows.len(), 3, "no assistant outside the main screen");
        let Some(Change::Config(on)) = &rows[0].change else {
            panic!()
        };
        let goodix = &on["touchCalibration"]["Goodix Capacitive TouchScreen"];
        assert_eq!(goodix["enabled"], true);
        assert_eq!(goodix["display"], "DSI-1");
        let other = &rows[2];
        assert_eq!(other.label, "ILITEK ILITEK-TP");
        let Action::SetConfig { patch } = &other.tools[0].action else {
            panic!()
        };
        assert_eq!(
            patch["touchCalibration"]["ILITEK ILITEK-TP"]["display"],
            "DSI-1"
        );
    }

    #[test]
    fn a_screen_without_a_display_says_so() {
        let t = Strings::new("en");
        let rows = rows(Screen::Aux, &json!({ "system": { "displays": [] } }), &t);
        assert_eq!(rows[0].label, "No touchscreen");
    }

    #[test]
    fn four_taps_give_the_matrix_that_puts_them_on_the_targets() {
        let off = |(x, y): (f64, f64)| (x * 0.9 + 0.03, y * 1.1 - 0.02);
        let touched: Vec<(f64, f64)> = TARGETS.iter().map(|&p| off(p)).collect();
        let m = solve(&touched).unwrap();
        for (&(u, v), &(tx, ty)) in touched.iter().zip(&TARGETS) {
            assert!((m[0] * u + m[1] * v + m[2] - tx).abs() < 1e-9);
            assert!((m[3] * u + m[4] * v + m[5] - ty).abs() < 1e-9);
        }
        let id = solve(&TARGETS).unwrap();
        for (got, want) in id.iter().zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]) {
            assert!((got - want).abs() < 1e-9);
        }
        assert!(solve(&[(0.5, 0.5); 4]).is_none());
        assert!(solve(&touched[..3]).is_none());
    }
}
