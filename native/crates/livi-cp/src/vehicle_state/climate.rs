// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn climate() -> Accessory {
    use Format::*;
    let on = |iid, name, initial| shared(iid, 0x3000_0002, name, Bool, Value::Bool(initial));
    let seat = |iid, key| {
        car(
            iid,
            0x3600_0065,
            "vehicleLayoutKey",
            String,
            Value::str(key),
        )
        .fixed()
    };
    let zone = |iid, names: [&'static str; 3], key| Service {
        iid,
        kind: 0x1100_0002,
        characteristics: vec![
            on(iid + 1, names[0], true),
            shared(iid + 2, 0x3100_0017, names[1], Float, Value::Real(21.0))
                .between(TEMPERATURE_LO, TEMPERATURE_HI, 0.5)
                .in_units(CELSIUS),
            car(iid + 3, 0x3100_0019, names[2], Float, Value::Real(21.0))
                .between(-40.0, 85.0, 0.5)
                .in_units(CELSIUS),
            seat(iid + 4, key),
        ],
    };
    Accessory {
        name: "climate",
        iid: 512,
        kind: 0x0100_0001,
        services: vec![
            Service {
                iid: 513,
                kind: 0x1100_0001,
                characteristics: vec![
                    shared(HVAC_ON, 0x3100_0002, "hvacOn", Bool, Value::Bool(true)),
                    shared(
                        COMPRESSOR,
                        0x3100_0003,
                        "compressorOn",
                        Bool,
                        Value::Bool(true),
                    ),
                    shared(MAX_AC, 0x3100_0022, "maxACOn", Bool, Value::Bool(false)),
                    shared(
                        MAX_DEFROST,
                        0x3100_0029,
                        "maxDefrostOn",
                        Bool,
                        Value::Bool(false),
                    ),
                ],
            },
            zone(
                518,
                [
                    "driverTemperatureOn",
                    "driverTargetTemperature",
                    "driverCurrentTemperature",
                ],
                "seat_front_left",
            ),
            zone(
                532,
                [
                    "passengerTemperatureOn",
                    "passengerTargetTemperature",
                    "passengerCurrentTemperature",
                ],
                "seat_front_right",
            ),
            Service {
                iid: 523,
                kind: 0x1100_0007,
                characteristics: vec![
                    on(524, "fanOn", true),
                    shared(525, 0x3100_0012, "fanLevel", Uint8, Value::Int(3)).up_to(7),
                    shared(
                        FAN_AUTO,
                        0x3000_005F,
                        "fanAutoMode",
                        Bool,
                        Value::Bool(true),
                    ),
                    seat(531, "seat_front"),
                ],
            },
            Service {
                iid: 527,
                kind: 0x1100_0011,
                characteristics: vec![on(RECIRCULATION, "recirculationOn", false)],
            },
            Service {
                iid: 529,
                kind: 0x1100_0008,
                characteristics: vec![on(AUTO_CLIMATE, "autoClimateOn", true)],
            },
            Service {
                iid: 537,
                kind: 0x1100_0012,
                characteristics: vec![
                    on(ZONES_SYNCED, "zonesSynced", false),
                    seat(539, "seat_front"),
                ],
            },
            window_heating(WINDOWS[0], "frontWindowHeatingLevel", "windshield_front"),
            window_heating(WINDOWS[1], "rearWindowHeatingLevel", "windshield_rear"),
            vent(VENTS[0], ["upperVentOn", "upperVentAutoMode"], UPPER, false),
            vent(
                VENTS[1],
                ["middleVentOn", "middleVentAutoMode"],
                MIDDLE,
                true,
            ),
            vent(VENTS[2], ["lowerVentOn", "lowerVentAutoMode"], LOWER, false),
        ],
    }
}

pub(super) fn vent(iid: i64, names: [&'static str; 2], direction: i64, on: bool) -> Service {
    use Format::*;
    Service {
        iid,
        kind: 0x1100_0006,
        characteristics: vec![
            shared(iid + 1, 0x3000_0002, names[0], Bool, Value::Bool(on)),
            shared(iid + 2, 0x3000_005F, names[1], Bool, Value::Bool(true)),
            car(
                iid + 3,
                0x3100_0025,
                "ventCombinations",
                Array,
                Value::Array(vec![Value::Int(direction)]),
            )
            .fixed(),
            car(
                iid + 4,
                0x3600_0065,
                "vehicleLayoutKey",
                String,
                Value::str("seat_front"),
            )
            .fixed(),
        ],
    }
}

pub(super) const UPPER: i64 = 2;
pub(super) const LOWER: i64 = 4;
pub(super) const MIDDLE: i64 = 16;
pub(super) const VENTS: [i64; 3] = [548, 553, 558];
pub(super) const AUTO_CLIMATE: i64 = 530;
pub(super) const FAN_AUTO: i64 = 526;
pub(super) const HVAC_ON: i64 = 514;
pub(super) const COMPRESSOR: i64 = 515;
pub(super) const MAX_AC: i64 = 516;
pub(super) const MAX_DEFROST: i64 = 517;
pub(super) const RECIRCULATION: i64 = 528;
pub(super) const WINDOWS: [i64; 2] = [540, 544];
pub(super) const TEMPERATURE_LO: f64 = 16.0;
pub(super) const TEMPERATURE_HI: f64 = 28.0;

pub(super) fn window_heating(iid: i64, name: &'static str, key: &'static str) -> Service {
    use Format::*;
    const FILAMENT: i64 = 2;
    Service {
        iid,
        kind: 0x1100_0005,
        characteristics: vec![
            shared(iid + 1, 0x3000_0004, name, Uint8, Value::Int(0)).up_to(1),
            car(
                iid + 2,
                0x3100_0014,
                "defrostTypes",
                Uint8,
                Value::Int(FILAMENT),
            )
            .up_to(7)
            .fixed(),
            car(
                iid + 3,
                0x3600_0065,
                "vehicleLayoutKey",
                String,
                Value::str(key),
            )
            .fixed(),
        ],
    }
}
