// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Engines {
    pub combustion: Option<Rpm>,
    pub electric: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rpm {
    pub max: u32,
    pub redline: u32,
}

pub(super) const ENGINE_RPM: i64 = 2562;
pub(super) const ENGINE_POWER: i64 = 2818;
pub(super) const COOLANT: i64 = 2566;

pub(super) fn combustion_engine(rpm: Rpm) -> Accessory {
    use Format::*;
    let max = rpm.max.max(1);
    let speed = |iid, kind, name, initial: u32| {
        car(iid, kind, name, Uint32, Value::Int(initial.min(max).into()))
            .up_to(max)
            .in_units(REVOLUTIONS_PER_MINUTE)
    };
    let temperature = |iid, kind, name, initial: f64| {
        car(iid, kind, name, Float, Value::Real(initial))
            .between(-40.0, 200.0, 1.0)
            .in_units(CELSIUS)
    };
    Accessory {
        name: "combustion engine",
        iid: 2560,
        kind: 0x0900_0005,
        services: vec![
            Service {
                iid: 2561,
                kind: 0x1A00_000F,
                characteristics: vec![
                    speed(ENGINE_RPM, 0x3000_0024, "rotationalSpeed", 0),
                    speed(2563, 0x3000_004E, "rotationalSpeedMax", max).fixed(),
                    speed(
                        2564,
                        0x3000_004F,
                        "rotationalSpeedMarkerRedline",
                        rpm.redline,
                    )
                    .fixed(),
                ],
            },
            Service {
                iid: COOLANT - 1,
                kind: 0x1A00_0010,
                characteristics: vec![
                    temperature(COOLANT, 0x3000_001D, "temperature", 0.0),
                    temperature(COOLANT + 1, 0x3000_0054, "temperatureMin", 40.0).fixed(),
                    temperature(COOLANT + 2, 0x3000_0055, "temperatureMax", 130.0).fixed(),
                    temperature(COOLANT + 3, 0x3000_0056, "temperatureMarkerCold", 60.0).fixed(),
                    temperature(COOLANT + 4, 0x3000_0057, "temperatureMarkerHot", 115.0).fixed(),
                ],
            },
        ],
    }
}

pub(super) fn electric_engine() -> Accessory {
    use Format::*;
    Accessory {
        name: "electric engine",
        iid: 2816,
        kind: 0x0900_0004,
        services: vec![Service {
            iid: 2817,
            kind: 0x1A00_000D,
            characteristics: vec![
                car(ENGINE_POWER, 0x3000_0028, "power", Float, Value::Real(0.0))
                    .between(-1000.0, 1000.0, 0.1)
                    .in_units(KILOWATTS),
            ],
        }],
    }
}
