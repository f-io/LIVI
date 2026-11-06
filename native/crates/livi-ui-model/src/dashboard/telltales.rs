// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::State;
use serde_json::Value;

use super::readings::Readings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Red,
    Amber,
    Green,
    Blue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lamp {
    pub icon: &'static str,
    pub glyph: &'static str,
    pub tint: Tint,
    pub blink: bool,
}

#[derive(Clone, Copy)]
enum On {
    Tell(&'static str),
    Hazards,
    ParkingBrake,
    OilHot,
    FuelLow,
    Lights,
    HighBeam,
}

const LAMPS: [(On, &str, &str, Tint); 28] = [
    (On::Tell("brake"), "TelltaleBrake", "", Tint::Red),
    (On::ParkingBrake, "TelltaleParkingRing", "P", Tint::Red),
    (On::Tell("seatBelt"), "TelltaleSeatBelt", "", Tint::Red),
    (On::Tell("airbag"), "TelltaleAirbag", "", Tint::Red),
    (On::Tell("sideAirbag"), "TelltaleSideAirbag", "", Tint::Red),
    (
        On::Tell("oilPressure"),
        "TelltaleOilPressure",
        "",
        Tint::Red,
    ),
    (On::Tell("coolant"), "TelltaleCoolant", "", Tint::Red),
    (On::OilHot, "DeviceThermostat", "", Tint::Red),
    (On::Tell("charging"), "TelltaleCharging", "", Tint::Red),
    (On::Hazards, "TelltaleHazard", "", Tint::Red),
    (On::Tell("abs"), "TelltaleParkingRing", "ABS", Tint::Amber),
    (On::Tell("esc"), "TelltaleEsc", "", Tint::Amber),
    (
        On::Tell("tyrePressure"),
        "TelltaleTyrePressure",
        "",
        Tint::Amber,
    ),
    (On::Tell("engine"), "TelltaleEngine", "", Tint::Amber),
    (On::Tell("brakeWear"), "TelltaleBrakeWear", "", Tint::Amber),
    (On::Tell("preheat"), "TelltalePreheat", "", Tint::Amber),
    (On::Tell("choke"), "TelltaleChoke", "", Tint::Amber),
    (On::FuelLow, "LocalGasStationOutlined", "", Tint::Amber),
    (On::Tell("rearFog"), "TelltaleRearFog", "", Tint::Amber),
    (
        On::Tell("passengerAirbagOff"),
        "TelltalePassengerAirbagOff",
        "",
        Tint::Amber,
    ),
    (
        On::Tell("windscreenDefrost"),
        "TelltaleWindscreenDefrost",
        "",
        Tint::Amber,
    ),
    (
        On::Tell("rearDefrost"),
        "TelltaleRearDefrost",
        "",
        Tint::Amber,
    ),
    (On::Tell("position"), "TelltalePosition", "", Tint::Green),
    (
        On::Tell("parkingLamps"),
        "TelltaleParkingLamps",
        "",
        Tint::Green,
    ),
    (On::Lights, "TelltaleLowBeam", "", Tint::Green),
    (On::Tell("frontFog"), "TelltaleFrontFog", "", Tint::Green),
    (
        On::Tell("autoHighBeam"),
        "TelltaleAutoHighBeam",
        "",
        Tint::Green,
    ),
    (On::HighBeam, "TelltaleHighBeam", "", Tint::Blue),
];

pub fn lamps(state: &State, r: &Readings) -> Vec<Lamp> {
    let tells = state.telemetry.get("telltales");
    let tell = |key: &str| tells.and_then(|t| t.get(key)).and_then(Value::as_bool) == Some(true);
    LAMPS
        .iter()
        .filter(|(on, ..)| {
            r.test
                || match on {
                    On::Tell(key) => tell(key),
                    On::Hazards => r.hazards,
                    On::ParkingBrake => r.parking_brake,
                    On::OilHot => r.oil_hot,
                    On::FuelLow => r.fuel_low,
                    On::Lights => r.lights,
                    On::HighBeam => r.high_beam,
                }
        })
        .map(|(on, icon, glyph, tint)| Lamp {
            icon,
            glyph,
            tint: *tint,
            blink: matches!(on, On::Hazards),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use livi_core_proto::state::Screen;
    use serde_json::json;

    use super::*;
    use crate::dashboard::readings::readings;

    fn shown(telemetry: Value) -> Vec<Lamp> {
        let s = crate::testing::state(json!({ "telemetry": telemetry }));
        lamps(&s, &readings(&s, Screen::Main))
    }

    #[test]
    fn every_contract_telltale_has_a_lamp() {
        let contract = serde_json::to_value(livi_core_proto::telemetry::Telltales::default())
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let lamps = LAMPS
            .iter()
            .filter_map(|(on, ..)| match on {
                On::Tell(key) => Some(key.to_string()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(contract, lamps);
    }

    #[test]
    fn a_dash_test_lights_every_lamp() {
        assert_eq!(shown(json!({ "dashTest": true })).len(), LAMPS.len());
    }

    #[test]
    fn nothing_lights_without_data() {
        assert!(shown(json!({})).is_empty());
    }

    #[test]
    fn lamps_light_red_first_and_blue_last() {
        let lit = shown(json!({
            "highBeam": true,
            "hazards": true,
            "fuelPct": 5,
            "telltales": { "seatBelt": true, "abs": true, "position": true, "esc": false }
        }));
        let icons: Vec<_> = lit.iter().map(|l| (l.icon, l.glyph)).collect();
        assert_eq!(
            icons,
            [
                ("TelltaleSeatBelt", ""),
                ("TelltaleHazard", ""),
                ("TelltaleParkingRing", "ABS"),
                ("LocalGasStationOutlined", ""),
                ("TelltalePosition", ""),
                ("TelltaleHighBeam", "")
            ]
        );
        assert!(lit[1].blink && !lit[0].blink);
        assert_eq!(lit.last().map(|l| l.tint), Some(Tint::Blue));
    }
}
