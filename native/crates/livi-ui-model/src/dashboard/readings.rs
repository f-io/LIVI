// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::{CarType, SpeedUnit, TemperatureUnit};
use livi_core_proto::state::{Screen, State};
use serde_json::Value;

use super::gauge::{Scale, scale};

const SPEED_MAX_SECTIONS: usize = 15;
const RPM_MAX_SECTIONS: usize = 20;
const RPM_STEP: f64 = 1000.0;
const FUEL_WARN_BELOW: i32 = 12;
const OIL_MIN_C: f64 = 40.0;
const OIL_MAX_C: f64 = 150.0;
const OIL_WARN_C: f64 = 125.0;
const MPH_PER_KPH: f64 = 0.621_371;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Turn {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Readings {
    pub enabled: bool,
    pub speed: f64,
    pub speed_text: String,
    pub speed_label: &'static str,
    pub speed_scale: Scale,
    pub rpm: f64,
    pub rpm_scale: Scale,
    pub redline: f64,
    pub gear: String,
    pub turn: Turn,
    pub hazards: bool,
    pub lights: bool,
    pub high_beam: bool,
    pub parking_brake: bool,
    pub ambient: Option<String>,
    pub fuel: i32,
    pub fuel_low: bool,
    pub battery: bool,
    pub oil_text: String,
    pub oil_ratio: f64,
    pub oil_hot: bool,
}

pub fn gear(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.trim().to_uppercase(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    match s.as_str() {
        "" | "UNKNOWN" => "—".into(),
        "NEUTRAL" => "N".into(),
        "REVERSE" => "R".into(),
        "PARK" => "P".into(),
        "DRIVE" => "D".into(),
        "WINTER" => "W".into(),
        "SPORT" => "S".into(),
        _ => s,
    }
}

fn temperature(c: f64, unit: TemperatureUnit) -> f64 {
    match unit {
        TemperatureUnit::Celsius => c,
        TemperatureUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
    }
}

fn speed(kph: f64, unit: SpeedUnit) -> f64 {
    match unit {
        SpeedUnit::Kmh => kph,
        SpeedUnit::Mph => kph * MPH_PER_KPH,
    }
}

pub fn readings(state: &State, screen: Screen) -> Readings {
    let c = &state.config;
    let t = |key: &str| state.telemetry.get(key).unwrap_or(&Value::Null);
    let num = |key: &str| t(key).as_f64().filter(|f| f.is_finite());
    let enabled = *c.dashboard.get(screen);
    let unit = c.speed_unit;
    let temp_unit = c.temperature_unit;
    let speed_scale = scale(
        speed(f64::from(c.max_speed_kph), unit),
        f64::from(c.speed_scale_step),
        SPEED_MAX_SECTIONS,
        |v| format!("{v}"),
    );
    let rpm_scale = scale(f64::from(c.max_rpm), RPM_STEP, RPM_MAX_SECTIONS, |v| {
        format!("{}", v / 1000.0)
    });
    let shown_speed = speed(num("speedKph").unwrap_or(0.0), unit);
    let fuel = num("fuelPct").unwrap_or(0.0).round().clamp(0.0, 100.0) as i32;
    let oil = num("oilC").unwrap_or(0.0);
    let oil_round = oil.round();
    let turn = match t("turnSignal").as_str() {
        Some("left") => Turn::Left,
        Some("right") => Turn::Right,
        _ => Turn::None,
    };
    let suffix = match temp_unit {
        TemperatureUnit::Celsius => "°C",
        TemperatureUnit::Fahrenheit => "°F",
    };
    Readings {
        enabled,
        speed: shown_speed,
        speed_text: (shown_speed.round() as i64).clamp(0, 999).to_string(),
        speed_label: match unit {
            SpeedUnit::Kmh => "KPH",
            SpeedUnit::Mph => "MPH",
        },
        speed_scale,
        rpm: num("rpm").unwrap_or(0.0),
        rpm_scale,
        redline: f64::from(c.redline_rpm),
        gear: gear(
            state
                .telemetry
                .get("gear")
                .unwrap_or(&Value::String("P".into())),
        ),
        turn,
        hazards: t("hazards").as_bool() == Some(true),
        lights: t("lights").as_bool() == Some(true),
        high_beam: t("highBeam").as_bool() == Some(true),
        parking_brake: t("parkingBrake").as_bool() == Some(true),
        ambient: num("ambientC").map(|a| format!("{}{suffix}", temperature(a, temp_unit).round())),
        fuel,
        fuel_low: fuel <= FUEL_WARN_BELOW,
        battery: c.car_type == Some(CarType::Electric),
        oil_text: format!("{}°", temperature(oil, temp_unit).round()),
        oil_ratio: (oil_round.clamp(OIL_MIN_C, OIL_MAX_C) - OIL_MIN_C) / (OIL_MAX_C - OIL_MIN_C),
        oil_hot: oil_round >= OIL_WARN_C,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state(telemetry: Value, config: Value) -> State {
        crate::testing::state(json!({ "config": config, "telemetry": telemetry }))
    }

    #[test]
    fn gears_read_short() {
        assert_eq!(gear(&json!("reverse")), "R");
        assert_eq!(gear(&json!(3)), "3");
        assert_eq!(gear(&json!(null)), "—");
    }

    #[test]
    fn speed_and_temperatures_follow_the_units() {
        let s = state(
            json!({ "speedKph": 100.0, "ambientC": 20.0, "oilC": 90.0 }),
            json!({ "speedUnit": "mph", "temperatureUnit": "fahrenheit" }),
        );
        let r = readings(&s, Screen::Main);
        assert_eq!(r.speed_text, "62");
        assert_eq!(r.speed_label, "MPH");
        assert_eq!(r.ambient.as_deref(), Some("68°F"));
        assert_eq!(r.oil_text, "194°");
        assert!(!r.oil_hot);
    }

    #[test]
    fn low_fuel_and_hot_oil_warn() {
        let s = state(json!({ "fuelPct": 10.4, "oilC": 130.0 }), json!({}));
        let r = readings(&s, Screen::Main);
        assert_eq!(r.fuel, 10);
        assert!(r.fuel_low && r.oil_hot);
        assert_eq!(r.gear, "P");
    }
}
