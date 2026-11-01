// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn seats() -> Accessory {
    use Format::*;
    let heating = |iid, names: [&'static str; 2], key| Service {
        iid,
        kind: 0x2200_0001,
        characteristics: vec![
            shared(iid + 1, 0x3100_0021, names[0], Int32, Value::Int(0)).up_to(2),
            shared(iid + 2, 0x3000_0002, names[1], Bool, Value::Bool(false)),
            car(
                iid + 3,
                0x3600_0065,
                "vehicleLayoutKey",
                String,
                Value::str(key),
            )
            .fixed(),
        ],
    };
    Accessory {
        name: "seats",
        iid: 1024,
        kind: 0x0150_0001,
        services: vec![
            heating(
                1025,
                ["driverSeatHeatingLevel", "driverSeatHeatingOn"],
                "seat_front_left",
            ),
            heating(
                1029,
                ["passengerSeatHeatingLevel", "passengerSeatHeatingOn"],
                "seat_front_right",
            ),
        ],
    }
}

#[derive(Clone, Copy)]
pub(super) struct Distance {
    pub(super) km: i64,
    pub(super) hidden: Option<i64>,
    pub(super) names: [&'static str; 3],
}

pub(super) const FUEL_LEVEL: i64 = 1538;
pub(super) const FUEL_RANGE: Distance = Distance {
    km: 1541,
    hidden: Some(1543),
    names: ["fuelRangeKm", "fuelRangeMiles", "fuelRangeHidden"],
};
pub(super) const FUEL_EFFICIENCY: i64 = 1545;
pub(super) const AVERAGE_FUEL_EFFICIENCY: i64 = 1546;
pub(super) const REMAINING_RANGE: Distance = Distance {
    km: 1794,
    hidden: Some(1796),
    names: [
        "remainingRangeKm",
        "remainingRangeMiles",
        "remainingRangeHidden",
    ],
};
pub(super) const ODOMETER: Distance = Distance {
    km: 1798,
    hidden: None,
    names: ["odometerKm", "odometerMiles", ""],
};
pub(super) const LOW_FUEL_PCT: i64 = 10;
pub(super) const KM_PER_MILE: f64 = 1.609_344;

pub(super) fn distance(iid: i64, kind: i64, d: Distance) -> Service {
    use Format::*;
    let mut characteristics = vec![
        car(d.km, 0x3000_004A, d.names[0], Uint64, Value::Int(0)).in_units(KILOMETERS),
        car(d.km + 1, 0x3000_004B, d.names[1], Uint64, Value::Int(0)).in_units(MILES),
    ];
    if let Some(hidden) = d.hidden {
        characteristics.push(car(
            hidden,
            0x3600_0023,
            d.names[2],
            Bool,
            Value::Bool(true),
        ));
    }
    Service {
        iid,
        kind,
        characteristics,
    }
}

pub(super) fn fuel() -> Accessory {
    use Format::*;
    Accessory {
        name: "fuel",
        iid: 1536,
        kind: 0x0900_0006,
        services: vec![
            Service {
                iid: 1537,
                kind: 0x1A00_0005,
                characteristics: vec![
                    car(FUEL_LEVEL, 0x4100_0002, "fuelLevel", Uint8, Value::Int(0))
                        .up_to(100)
                        .in_units(PERCENT),
                    car(
                        1539,
                        0x4100_001A,
                        "fuelLevelMarkerLow",
                        Uint8,
                        Value::Int(LOW_FUEL_PCT),
                    )
                    .up_to(100)
                    .in_units(PERCENT)
                    .fixed(),
                ],
            },
            distance(1540, 0x1A00_0015, FUEL_RANGE),
            Service {
                iid: 1544,
                kind: 0x1A00_0014,
                characteristics: vec![
                    car(
                        FUEL_EFFICIENCY,
                        0x3500_0013,
                        "fuelEfficiency",
                        Uint32,
                        Value::Int(0),
                    )
                    .in_units(MILLILITERS_PER_100_KM),
                    car(
                        AVERAGE_FUEL_EFFICIENCY,
                        0x3500_0018,
                        "averageFuelEfficiency",
                        Uint32,
                        Value::Int(0),
                    )
                    .in_units(MILLILITERS_PER_100_KM),
                    car(
                        1547,
                        0x4100_0025,
                        "fuelEfficiencyMax",
                        Uint32,
                        Value::Int(50_000),
                    )
                    .in_units(MILLILITERS_PER_100_KM)
                    .fixed(),
                ],
            },
        ],
    }
}

pub(super) fn trip_computer() -> Accessory {
    Accessory {
        name: "trip computer",
        iid: 1792,
        kind: 0x0400_0001,
        services: vec![
            distance(1793, 0x1A00_000B, REMAINING_RANGE),
            distance(1797, 0x1A00_000A, ODOMETER),
        ],
    }
}

pub(super) const WHEELS: [(&str, &str); 4] = [
    ("frontLeftKpa", "tire_front_left"),
    ("frontRightKpa", "tire_front_right"),
    ("rearLeftKpa", "tire_rear_left"),
    ("rearRightKpa", "tire_rear_right"),
];
pub(super) const TIRES: i64 = 2048;
pub(super) const PRESSURE_UNDEFINED: i64 = 0;
pub(super) const SENSOR_ACTIVE: i64 = 1;
pub(super) const SENSOR_WAITING: i64 = 3;

pub(super) fn tire_pressure_iid(wheel: usize) -> i64 {
    TIRES + 2 + 5 * wheel as i64
}

pub(super) fn tires() -> Accessory {
    use Format::*;
    let wheel = |n: usize, key: &'static str| {
        let iid = tire_pressure_iid(n);
        Service {
            iid: iid - 1,
            kind: 0x1B00_0001,
            characteristics: vec![
                car(iid, 0x3000_001B, "tirePressureKpa", Uint32, Value::Int(0))
                    .between(0.0, 1000.0, 1.0)
                    .in_units(KILOPASCALS),
                car(
                    iid + 1,
                    0x3600_0065,
                    "vehicleLayoutKey",
                    String,
                    Value::str(key),
                )
                .fixed(),
                car(
                    iid + 2,
                    0x3000_001C,
                    "tirePressureState",
                    Uint8,
                    Value::Int(PRESSURE_UNDEFINED),
                )
                .up_to(5),
                car(
                    iid + 3,
                    0x3000_001A,
                    "tireSensorState",
                    Uint8,
                    Value::Int(SENSOR_WAITING),
                )
                .up_to(4),
            ],
        }
    };
    Accessory {
        name: "tires",
        iid: TIRES,
        kind: 0x0A00_0001,
        services: WHEELS
            .iter()
            .enumerate()
            .map(|(n, (_, key))| wheel(n, key))
            .collect(),
    }
}
