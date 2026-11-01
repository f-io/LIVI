// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! What the MiniDSP page shows, read from `state["minidsp"]` and
//! `state["config"]["minidsp"]`.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Control {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Control {
    /// One step down (-1) or up (1), kept in range and free of float dust.
    pub fn nudge(&self, direction: i32) -> f64 {
        let next = self.value + f64::from(direction) * self.step;
        ((next * 100.0).round() / 100.0).clamp(self.min, self.max)
    }

    /// Position 0..1 on the slider track.
    pub fn level(&self) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        ((self.value - self.min) / span).clamp(0.0, 1.0) as f32
    }

    /// The value at a slider position, snapped to the step.
    pub fn at_level(&self, level: f32) -> f64 {
        let raw = self.min + f64::from(level.clamp(0.0, 1.0)) * (self.max - self.min);
        let snapped = if self.step > 0.0 {
            self.min + ((raw - self.min) / self.step).round() * self.step
        } else {
            raw
        };
        ((snapped * 100.0).round() / 100.0).clamp(self.min, self.max)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub index: u32,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    Error,
    Waiting,
    Ready,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub enabled: bool,
    pub phase: Phase,
    pub busy: bool,
    pub error: String,
    pub product: String,
    pub source: String,
    pub volume: Control,
    pub bass: Control,
    /// One-based output numbers the bass control drives.
    pub bass_outputs: Vec<u32>,
    pub presets: Vec<Preset>,
}

fn num(v: &Value, default: f64) -> f64 {
    v.as_f64().filter(|f| f.is_finite()).unwrap_or(default)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

pub fn view(state: &Value) -> View {
    let dsp = &state["minidsp"];
    let c = &state["config"]["minidsp"];
    let connected = dsp["connected"].as_bool().unwrap_or(false);
    let error = text(&dsp["error"]);
    let phase = if !error.is_empty() {
        Phase::Error
    } else if !dsp["checked"].as_bool().unwrap_or(false) {
        Phase::Connecting
    } else if connected {
        Phase::Ready
    } else {
        Phase::Waiting
    };
    let volume_min = num(&c["volumeMinDb"], -80.0);
    let preset = dsp["preset"].as_u64();
    View {
        enabled: c["enabled"].as_bool().unwrap_or(false),
        phase,
        busy: dsp["busy"].as_bool().unwrap_or(false),
        error,
        product: text(&dsp["productName"]),
        source: text(&dsp["source"]),
        volume: Control {
            value: num(&dsp["volumeDb"], volume_min),
            min: volume_min,
            max: num(&c["volumeMaxDb"], 0.0),
            step: num(&c["volumeStepDb"], 0.5),
        },
        bass: Control {
            value: num(&c["bassGainDb"], 0.0),
            min: num(&c["bassMinDb"], -80.0),
            max: num(&c["bassMaxDb"], 12.0),
            step: num(&c["bassStepDb"], 0.5),
        },
        bass_outputs: c["bassOutputChannels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|i| i as u32 + 1)
            .collect(),
        presets: c["presets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let index = p["index"].as_u64()?;
                Some(Preset {
                    index: index as u32,
                    label: text(&p["label"]),
                    active: connected && preset == Some(index),
                })
            })
            .collect(),
    }
}

/// The caption under the page title.
pub fn status_line(v: &View, tr: impl Fn(&str, &str) -> String) -> String {
    match v.phase {
        Phase::Error => tr("minidsp.unavailable", "Unavailable"),
        Phase::Connecting => tr("minidsp.connecting", "Connecting…"),
        Phase::Waiting => tr("minidsp.notConnected", "Not connected"),
        Phase::Ready => [
            tr("minidsp.connected", "Connected"),
            v.product.clone(),
            v.source.clone(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · "),
    }
}

/// A gain as the page prints it, signed with one decimal.
pub fn db(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    if value > 0.0 {
        format!("+{value:.1}")
    } else {
        format!("{value:.1}")
    }
}

pub fn outputs(list: &[u32]) -> String {
    list.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn en(_: &str, text: &str) -> String {
        text.to_string()
    }

    fn config() -> Value {
        json!({
            "enabled": true,
            "volumeMinDb": -80.0, "volumeMaxDb": 0.0, "volumeStepDb": 0.5,
            "bassGainDb": 3.0, "bassMinDb": -80.0, "bassMaxDb": 12.0, "bassStepDb": 0.5,
            "bassOutputChannels": [2, 3],
            "presets": [{ "index": 0, "label": "Preset 1" }, { "index": 1, "label": "Night" }]
        })
    }

    #[test]
    fn the_page_waits_for_the_first_answer() {
        let v = view(&json!({ "minidsp": {}, "config": { "minidsp": config() } }));
        assert_eq!(v.phase, Phase::Connecting);
        assert_eq!(status_line(&v, en), "Connecting…");
        assert_eq!(v.volume.value, -80.0);
        assert!(v.presets.iter().all(|p| !p.active));
    }

    #[test]
    fn a_connected_device_fills_the_controls() {
        let state = json!({
            "minidsp": { "checked": true, "connected": true, "preset": 1, "source": "Toslink",
                         "volumeDb": -18.5, "productName": "2x4HD" },
            "config": { "minidsp": config() }
        });
        let v = view(&state);
        assert_eq!(v.phase, Phase::Ready);
        assert_eq!(status_line(&v, en), "Connected · 2x4HD · Toslink");
        assert_eq!(v.volume.value, -18.5);
        assert_eq!(v.bass.value, 3.0);
        assert_eq!(outputs(&v.bass_outputs), "3 + 4");
        assert_eq!(
            v.presets,
            vec![
                Preset {
                    index: 0,
                    label: "Preset 1".into(),
                    active: false
                },
                Preset {
                    index: 1,
                    label: "Night".into(),
                    active: true
                },
            ]
        );
    }

    #[test]
    fn errors_win_over_the_status() {
        let state = json!({
            "minidsp": { "checked": true, "connected": true, "error": "MiniDSP returned 500" },
            "config": { "minidsp": config() }
        });
        let v = view(&state);
        assert_eq!(v.phase, Phase::Error);
        assert_eq!(status_line(&v, en), "Unavailable");
        let gone =
            view(&json!({ "minidsp": { "checked": true }, "config": { "minidsp": config() } }));
        assert_eq!(gone.phase, Phase::Waiting);
        assert_eq!(status_line(&gone, en), "Not connected");
    }

    #[test]
    fn steps_and_the_slider_stay_in_range() {
        let c = Control {
            value: -0.25,
            min: -80.0,
            max: 0.0,
            step: 0.5,
        };
        assert_eq!(c.nudge(1), 0.0);
        assert_eq!(c.nudge(-1), -0.75);
        let low = Control { value: -80.0, ..c };
        assert_eq!(low.nudge(-1), -80.0);
        assert_eq!(c.at_level(0.0), -80.0);
        assert_eq!(c.at_level(1.0), 0.0);
        assert_eq!(c.at_level(0.503), -40.0);
        assert_eq!(Control { value: -40.0, ..c }.level(), 0.5);
        assert_eq!(
            Control {
                min: 1.0,
                max: 1.0,
                ..c
            }
            .level(),
            0.0
        );
    }

    #[test]
    fn gains_print_signed() {
        assert_eq!(db(1.5), "+1.5");
        assert_eq!(db(0.0), "0.0");
        assert_eq!(db(-0.0), "0.0");
        assert_eq!(db(-18.0), "-18.0");
    }
}
