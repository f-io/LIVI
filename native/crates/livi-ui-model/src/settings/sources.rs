// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

use crate::i18n::Strings;
use crate::settings::node::{Condition, OptionSource, Radio, Screen, UnitSource};

pub const KM_PER_MILE: f64 = 1.609344;
const SPEED_MAX_SECTIONS: f64 = 15.0;
const DEFAULT_SPEED_SCALE_STEP: f64 = 40.0;
const DONGLE_LINK: &str = "livi-link";

#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub label: String,
    pub value: Value,
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    pub factor: f64,
    pub suffix: &'static str,
    pub step: Option<f64>,
    pub max: Option<f64>,
}

pub fn holds(when: Condition, system: &Value) -> bool {
    match when {
        Condition::NotLinux => cfg!(not(target_os = "linux")),
        Condition::DisplaysSettable => system["displaysSettable"].as_bool() == Some(true),
        Condition::DisplaysNotSettable => system["displaysSettable"].as_bool() != Some(true),
    }
}

pub fn unit(source: UnitSource, config: &Value) -> Unit {
    match source {
        UnitSource::Speed => {
            let step = config["speedScaleStep"]
                .as_f64()
                .unwrap_or(DEFAULT_SPEED_SCALE_STEP);
            let mph = config["speedUnit"].as_str() == Some("mph");
            Unit {
                factor: if mph { 1.0 / KM_PER_MILE } else { 1.0 },
                suffix: if mph { "mph" } else { "km/h" },
                step: Some(step),
                max: Some(SPEED_MAX_SECTIONS * step),
            }
        }
    }
}

fn screen_key(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "main",
        Screen::Cluster => "cluster",
        Screen::Aux => "aux",
    }
}

pub(crate) fn display_for(state: &Value, screen: Screen) -> Option<&Value> {
    let displays = state["system"]["displays"].as_array()?;
    let key = screen_key(screen);
    let chosen = state["config"]["displayConnector"][key]
        .as_str()
        .unwrap_or("");
    displays
        .iter()
        .find(|d| {
            if chosen.is_empty() {
                d["screen"].as_str() == Some(key)
            } else {
                d["connector"].as_str() == Some(chosen)
            }
        })
        .or_else(|| (screen == Screen::Main && chosen.is_empty()).then(|| displays.first())?)
}

fn plain(label: impl Into<String>, value: Value) -> Offer {
    Offer {
        label: label.into(),
        value,
        offline: false,
    }
}

fn adapter(name: &str, switched_on: Option<bool>, model: Option<&str>, t: &Strings) -> Offer {
    if name != DONGLE_LINK {
        let label = match model {
            Some(m) => format!("{name} ({m})"),
            None => name.to_string(),
        };
        return plain(label, Value::from(name));
    }
    if switched_on == Some(false) {
        return plain(
            t.t("settings.dongleSwitchedOff", "LIVI Link (off)"),
            Value::from(name),
        );
    }
    plain("LIVI Link", Value::from(name))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn offers(source: OptionSource, state: &Value, t: &Strings) -> Vec<Offer> {
    let system = &state["system"];
    match source {
        OptionSource::AudioSinks | OptionSource::AudioSources => {
            let key = if source == OptionSource::AudioSinks {
                "audioSinks"
            } else {
                "audioSources"
            };
            system[key]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| Offer {
                    label: d["name"].as_str().unwrap_or("").to_string(),
                    value: d["id"].clone(),
                    offline: d["offline"].as_bool().unwrap_or(false),
                })
                .collect()
        }
        OptionSource::Connectors => system["displays"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| {
                let connector = d["connector"].as_str().unwrap_or("");
                let panel = d["panel"].as_str().unwrap_or("");
                let label = if panel.is_empty() {
                    connector.to_string()
                } else {
                    format!("{connector} · {panel}")
                };
                plain(label, Value::from(connector))
            })
            .collect(),
        OptionSource::DisplayModes(screen) => display_for(state, screen)
            .map(|d| strings(&d["modes"]))
            .unwrap_or_default()
            .into_iter()
            .map(|m| plain(m.clone(), Value::from(m)))
            .collect(),
        OptionSource::WifiBands => {
            let mut bands = strings(&system["wifiBands"]);
            if bands.is_empty() {
                bands = vec!["2.4ghz".into(), "5ghz".into()];
            }
            bands
                .into_iter()
                .map(|b| {
                    let label = match b.as_str() {
                        "2.4ghz" => "2.4 GHz",
                        "5ghz" => "5 GHz",
                        "6ghz" => "6 GHz",
                        other => other,
                    }
                    .to_string();
                    plain(label, Value::from(b))
                })
                .collect()
        }
        OptionSource::WifiChannels => system["wifiChannels"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| plain(c.to_string(), c.clone()))
            .collect(),
        OptionSource::WifiCountries => strings(&system["wifiCountries"])
            .into_iter()
            .map(|c| plain(c.clone(), Value::from(c)))
            .collect(),
        OptionSource::WifiInterfaces | OptionSource::BtAdapters => {
            let (list, radio) = if source == OptionSource::WifiInterfaces {
                ("wifiInterfaces", "wifi")
            } else {
                ("btAdapters", "bt")
            };
            strings(&system[list])
                .iter()
                .map(|i| {
                    adapter(
                        i,
                        system["dongle"][radio].as_bool(),
                        system["interfaceModels"][i.as_str()].as_str(),
                        t,
                    )
                })
                .collect()
        }
        OptionSource::Cameras => state["cameras"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                plain(
                    c["name"].as_str().unwrap_or("").to_string(),
                    c["id"].clone(),
                )
            })
            .collect(),
        OptionSource::DabGains => state["dab"]["gainsDb"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_f64())
            .map(|db| plain(format!("{db:.1} dB"), Value::from(db)))
            .collect(),
    }
}

pub fn radio_key(radio: Radio) -> &'static str {
    match radio {
        Radio::Wifi => "wifi",
        Radio::Bt => "bt",
    }
}

pub fn is_dongle(value: &Value) -> bool {
    value.as_str() == Some(DONGLE_LINK)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn display_modes_follow_the_chosen_connector() {
        let state = json!({
            "config": { "displayConnector": { "main": "HDMI-A-2" } },
            "system": { "displays": [
                { "connector": "HDMI-A-1", "screen": "main", "modes": ["800x480@60"] },
                { "connector": "HDMI-A-2", "screen": null, "modes": ["1920x1080@60"] }
            ] }
        });
        let modes = offers(
            OptionSource::DisplayModes(Screen::Main),
            &state,
            &Strings::default(),
        );
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].label, "1920x1080@60");
    }

    #[test]
    fn the_dongle_reads_as_livi_link() {
        let state = json!({ "system": { "wifiInterfaces": ["wlan0", "livi-link"], "dongle": { "wifi": false }, "interfaceModels": { "wlan0": "MT7925" } } });
        let list = offers(OptionSource::WifiInterfaces, &state, &Strings::default());
        assert_eq!(list[0].label, "wlan0 (MT7925)");
        assert_eq!(list[1].label, "LIVI Link (off)");
    }

    #[test]
    fn speed_scale_switches_with_the_unit() {
        let u = unit(
            UnitSource::Speed,
            &json!({ "speedUnit": "mph", "speedScaleStep": 20 }),
        );
        assert_eq!(u.suffix, "mph");
        assert_eq!(u.max, Some(300.0));
    }
}
