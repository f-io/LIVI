// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn navigation() -> Accessory {
    use Format::*;
    Accessory {
        name: "navigation",
        iid: 256,
        kind: 0x0E00_0002,
        services: vec![
            Service {
                iid: 257,
                kind: 0x1E00_0104,
                characteristics: vec![
                    phone(258, 0x4500_0103, "applicationEnabled", Bool),
                    phone(259, 0x4500_0112, "currentLegIndex", Uint32),
                    phone(260, 0x4500_0109, "legs", Array),
                    phone(261, 0x4500_010D, "sharingState", Uint8).up_to(4),
                    phone(262, 0x4500_0104, "userEnabled", Bool),
                    car(
                        263,
                        0x4500_010E,
                        "userVisibleReason",
                        String,
                        Value::str("LIVI"),
                    )
                    .fixed(),
                    car(264, 0x4500_0105, "vehicleEnabled", Bool, Value::Bool(true)),
                ],
            },
            Service {
                iid: 265,
                kind: 0x1E00_0102,
                characteristics: vec![
                    phone(266, 0x4500_0108, "destination", Dict),
                    phone(267, 0x4500_0106, "geodeticSystem", Uint8).up_to(2),
                    phone(268, 0x4500_0107, "origin", Dict),
                    phone(269, 0x4500_0110, "rerouteReason", Uint8).up_to(6),
                    phone(270, 0x4500_0101, "routeState", Uint8).up_to(255),
                    phone(271, 0x4500_0102, "userVisibleApplicationName", String),
                ],
            },
            Service {
                iid: 272,
                kind: 0x1E00_0103,
                characteristics: vec![
                    car(273, 0x4500_010C, "routeSource", Uint8, Value::Int(0)).up_to(5),
                    car(
                        274,
                        0x4500_010A,
                        "userVisibleNavigationSystemName",
                        String,
                        Value::str("LIVI"),
                    )
                    .fixed(),
                    car(
                        275,
                        0x4500_0111,
                        "userVisibleVehicleBrand",
                        String,
                        Value::str("LIVI"),
                    )
                    .fixed(),
                ],
            },
        ],
    }
}
