// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) const LIVI_SECTION: &str = "LIVI";

pub(super) fn info_line(
    iid: i64,
    id: &str,
    label: &str,
    value: &str,
    symbol: &str,
    order: i64,
) -> Service {
    use Format::*;
    let value = if value.is_empty() { "?" } else { value };
    Service {
        iid,
        kind: 0x1600_0015,
        characteristics: vec![
            car(
                iid + 1,
                0x3000_0019,
                "settingIdentifier",
                String,
                Value::str(id),
            )
            .fixed(),
            car(
                iid + 2,
                0x3000_0001,
                "settingLabel",
                String,
                Value::str(label),
            )
            .fixed(),
            car(
                iid + 3,
                0x3000_0017,
                "settingValue",
                String,
                Value::str(value),
            )
            .fixed(),
            car(
                iid + 4,
                0x3600_0006,
                "settingsCategory",
                Uint8,
                Value::Int(0),
            )
            .up_to(15)
            .fixed(),
            car(
                iid + 5,
                0x3000_005E,
                "settingSymbol",
                String,
                Value::str(symbol),
            )
            .fixed(),
            car(
                iid + 6,
                0x3000_0003,
                "settingSortOrder",
                Uint8,
                Value::Int(order),
            )
            .up_to(100)
            .fixed(),
            car(
                iid + 7,
                0x3600_0028,
                "settingSection",
                String,
                Value::str(LIVI_SECTION),
            )
            .fixed(),
        ],
    }
}

pub(super) fn automaker_settings(app_version: &str, build_date: &str) -> Accessory {
    use Format::*;
    Accessory {
        name: "automaker settings",
        iid: 1280,
        kind: 0x0500_0001,
        services: vec![
            info_line(
                VERSION_LINE,
                "LiviVersion",
                "Version",
                app_version,
                "info.circle",
                0,
            ),
            info_line(
                1289,
                "LiviVersionBuildDate",
                "Version Build",
                build_date,
                "hammer",
                1,
            ),
            Service {
                iid: 1297,
                kind: 0x1600_0020,
                characteristics: vec![
                    car(
                        1298,
                        0x3000_0019,
                        "sectionIdentifier",
                        String,
                        Value::str(LIVI_SECTION),
                    )
                    .fixed(),
                    car(1299, 0x3000_0003, "sectionSortOrder", Uint8, Value::Int(0))
                        .up_to(100)
                        .fixed(),
                    car(
                        1300,
                        0x3600_0004,
                        "sectionName",
                        String,
                        Value::str(LIVI_SECTION),
                    )
                    .fixed(),
                ],
            },
        ],
    }
}
