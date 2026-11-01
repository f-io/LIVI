// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notice {
    pub title: String,
    pub text: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryEntry {
    pub id: u64,
    pub notice: Notice,
    pub at_secs: u64,
    pub hidden: bool,
}

pub(super) fn history_slot(n: usize) -> i64 {
    HISTORY + 1 + 8 * n as i64
}

pub(super) fn notification_history() -> Accessory {
    use Format::*;
    let open = |iid, kind, name, format| characteristic(iid, kind, name, format, Owner::Car, None);
    let both =
        |iid, kind, name, format| characteristic(iid, kind, name, format, Owner::Shared, None);
    let services = (0..HISTORY_SLOTS)
        .map(|n| {
            let iid = history_slot(n);
            Service {
                iid,
                kind: 0x2100_0001,
                characteristics: vec![
                    open(iid + 1, 0x3000_0001, "historyLabel", String),
                    open(iid + 2, 0x3000_0005, "historyDescription", String),
                    open(iid + 3, 0x4900_0003, "historyTimestamp", Uint64)
                        .up_to(u32::MAX)
                        .in_units(SECONDS),
                    both(iid + 4, 0x3000_005E, "historySymbol", String),
                    both(iid + 5, 0x4900_0002, "historySeverity", Uint8).up_to(2),
                    both(iid + 6, 0x3600_0063, "historyDismissible", Bool),
                    both(iid + 7, 0x3600_0023, "historyHidden", Bool),
                ],
            }
        })
        .collect();
    Accessory {
        name: "notification history",
        iid: HISTORY,
        kind: 0x0140_0001,
        services,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterContent {
    pub panel: String,
    pub zones: [String; 2],
}

pub(super) const ZONES: [&str; 2] = ["zone1", "zone2"];

pub(super) fn zone_service(
    iid: i64,
    kind: i64,
    url_kind: i64,
    url: &str,
    on: Option<bool>,
    panel: &str,
    zone: &str,
) -> Service {
    use Format::*;
    let mut characteristics = vec![if kind == REQUEST_TEMPORARY_CONTENT {
        shared(iid + 1, url_kind, "contentURL", String, Value::str(url))
    } else {
        car(iid + 1, url_kind, "contentURL", String, Value::str(url))
    }];
    let mut next = iid + 2;
    if let Some(on) = on {
        characteristics.push(shared(next, 0x3000_0002, "on", Bool, Value::Bool(on)));
        next += 1;
    }
    if kind != REQUEST_TEMPORARY_CONTENT {
        characteristics.push(
            car(
                next,
                0x3600_0063,
                "userDismissible",
                Bool,
                Value::Bool(true),
            )
            .fixed(),
        );
        next += 1;
    }
    characteristics.extend([
        car(
            next,
            0x3700_0007,
            "displayPanelIdentifier",
            String,
            Value::str(panel),
        )
        .fixed(),
        car(
            next + 1,
            0x3700_000B,
            "displayZoneIdentifier",
            String,
            Value::str(zone),
        )
        .fixed(),
    ]);
    Service {
        iid,
        kind,
        characteristics,
    }
}

pub(super) fn notice_service(panel: &str) -> Service {
    use Format::*;
    let open = |iid, kind, name, format| characteristic(iid, kind, name, format, Owner::Car, None);
    Service {
        iid: NOTICE,
        kind: 0x1700_0005,
        characteristics: vec![
            open(NOTICE + 1, 0x3000_0001, "userVisibleLabel", String),
            open(NOTICE + 2, 0x3000_0005, "userVisibleDescription", String),
            open(NOTICE + 3, 0x3000_0019, "identifier", String),
            open(NOTICE + 4, 0x3700_0008, "symbolNameAndColor", Dict),
            car(
                NOTICE + 5,
                0x3700_0007,
                "displayPanelIdentifier",
                String,
                Value::str(panel),
            )
            .fixed(),
            car(
                NOTICE + 6,
                0x3700_000B,
                "displayZoneIdentifier",
                String,
                Value::str(ZONES[0]),
            )
            .fixed(),
            open(NOTICE + 7, 0x3700_000C, "trailingButton", Dict),
            characteristic(
                NOTICE + 8,
                0x3700_000D,
                "trailingButtonState",
                Uint8,
                Owner::Shared,
                None,
            ),
        ],
    }
}

pub(super) const NOTICE: i64 = 3360;
pub(super) const TEMPORARY_ZONE: i64 = 3085;
pub(super) const NOTICE_URL: &str = "minimalRemoteNotification://";
pub(super) const REQUEST_CONTENT: i64 = 0x1700_0004;
pub(super) const REQUEST_TEMPORARY_CONTENT: i64 = 0x1700_000C;
pub(super) const DYNAMIC_CONTENT_ELEMENT: i64 = 0x1700_000B;

pub(super) fn cluster_content(content: &ClusterContent) -> [Accessory; 2] {
    let panel = content.panel.as_str();
    let mut requests = Vec::new();
    for (n, zone) in ZONES.iter().enumerate() {
        let url = content.zones[n].as_str();
        let base = 3073 + 6 * n as i64;
        requests.push(zone_service(
            base,
            REQUEST_CONTENT,
            0x3000_0007,
            url,
            Some(!url.is_empty()),
            panel,
            zone,
        ));
    }
    for (n, zone) in ZONES.iter().enumerate() {
        let base = 3085 + 6 * n as i64;
        requests.push(zone_service(
            base,
            REQUEST_TEMPORARY_CONTENT,
            0x3600_0069,
            "",
            Some(false),
            panel,
            zone,
        ));
    }
    let mut elements: Vec<Service> = ZONES
        .iter()
        .enumerate()
        .map(|(n, zone)| {
            let base = 3329 + 6 * n as i64;
            zone_service(
                base,
                DYNAMIC_CONTENT_ELEMENT,
                0x3700_0013,
                "",
                None,
                panel,
                zone,
            )
        })
        .collect();
    elements.push(notice_service(panel));
    [
        Accessory {
            name: "cluster content",
            iid: 3072,
            kind: 0x0120_0001,
            services: requests,
        },
        Accessory {
            name: "cluster notifications",
            iid: 3328,
            kind: 0x0600_0001,
            services: elements,
        },
    ]
}
