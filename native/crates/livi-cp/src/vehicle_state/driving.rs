// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) const SPEED: i64 = 3842;
const MAX_KPH: u32 = 300;

pub(super) fn vehicle_motion() -> Accessory {
    use Format::*;
    let max_mph = (f64::from(MAX_KPH) / KM_PER_MILE).round() as u32;
    let speed = |iid, kind, name, max: u32, initial: u32| {
        car(iid, kind, name, Uint32, Value::Int(initial.into())).up_to(max)
    };
    Accessory {
        name: "vehicle motion",
        iid: 3840,
        kind: 0x0900_0001,
        services: vec![Service {
            iid: 3841,
            kind: 0x1A00_0009,
            characteristics: vec![
                speed(SPEED, 0x3000_0046, "speedKmh", MAX_KPH, 0),
                speed(SPEED + 1, 0x3000_0047, "speedMph", max_mph, 0),
                speed(SPEED + 2, 0x3000_0048, "speedMaxKmh", MAX_KPH, MAX_KPH).fixed(),
                speed(SPEED + 3, 0x3000_0049, "speedMaxMph", max_mph, max_mph).fixed(),
            ],
        }],
    }
}

pub(super) const TURN_SIGNALS: i64 = 4610;
pub(super) const SIGNAL_OFF: i64 = 0;
pub(super) const SIGNAL_ON: i64 = 1;

pub(super) fn indicators() -> Accessory {
    use Format::*;
    let signal = |iid, kind, name| car(iid, kind, name, Uint8, Value::Int(SIGNAL_OFF)).up_to(1);
    Accessory {
        name: "indicators",
        iid: 4608,
        kind: 0x0190_0001,
        services: vec![Service {
            iid: 4609,
            kind: 0x2400_0001,
            characteristics: vec![
                signal(TURN_SIGNALS, 0x5100_0001, "turnSignalLeft"),
                signal(TURN_SIGNALS + 1, 0x5100_0002, "turnSignalRight"),
            ],
        }],
    }
}

pub(super) const CLOSURES: [(&str, &str); 6] = [
    ("frontLeft", "door_front_left"),
    ("frontRight", "door_front_right"),
    ("rearLeft", "door_rear_left"),
    ("rearRight", "door_rear_right"),
    ("hood", "hood"),
    ("trunk", "trunk"),
];
const CLOSURE_ACCESSORY: i64 = 4864;
pub(super) const LATCHED: bool = true;

pub(super) fn latch_iid(closure: usize) -> i64 {
    CLOSURE_ACCESSORY + 2 + 3 * closure as i64
}

pub(super) fn closures() -> Accessory {
    use Format::*;
    let closure = |n: usize, key: &'static str| {
        let iid = latch_iid(n);
        Service {
            iid: iid - 1,
            kind: 0x1D00_0001,
            characteristics: vec![
                car(iid, 0x4200_000B, "latchState", Bool, Value::Bool(LATCHED)),
                car(
                    iid + 1,
                    0x3600_0065,
                    "vehicleLayoutKey",
                    String,
                    Value::str(key),
                )
                .fixed(),
            ],
        }
    };
    Accessory {
        name: "closures",
        iid: CLOSURE_ACCESSORY,
        kind: 0x0D00_0001,
        services: CLOSURES
            .iter()
            .enumerate()
            .map(|(n, (_, key))| closure(n, key))
            .collect(),
    }
}
