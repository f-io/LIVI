// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) const INTERIOR_STYLE: i64 = 770;
pub(super) const EXTERIOR_STYLE: i64 = 771;
pub(super) const VEHICLE_LAYOUT: i64 = 772;
pub(super) const IMAGE_ARCHIVE_SERVICE: i64 = 776;
pub(super) const GET_IMAGE_ARCHIVE: i64 = 778;
pub(super) const LOGO: &str = "livi";
pub(super) const HISTORY: i64 = 3584;
pub(super) const HISTORY_SLOTS: usize = 10;
pub(super) const VERSION_LINE: i64 = 1281;

pub(super) const VEHICLE_RESOURCES: i64 = 768;

pub(super) fn initial_values(accessories: &[Accessory]) -> impl Iterator<Item = (i64, Value)> + '_ {
    accessories
        .iter()
        .flat_map(|a| &a.services)
        .flat_map(|s| &s.characteristics)
        .filter_map(|c| Some((c.iid, c.initial.clone()?)))
}

pub(super) fn vehicle_resources() -> Accessory {
    use Format::*;
    Accessory {
        name: "vehicle resources",
        iid: VEHICLE_RESOURCES,
        kind: 0x0120_0002,
        services: vec![Service {
            iid: 769,
            kind: 0x2000_0001,
            characteristics: vec![
                car(
                    INTERIOR_STYLE,
                    0x4800_0004,
                    "interiorStyle",
                    String,
                    Value::str("5Seater"),
                )
                .fixed(),
            ],
        }],
    }
}

pub(super) fn fetched_archives() -> std::sync::MutexGuard<'static, BTreeSet<String>> {
    static FETCHED: std::sync::Mutex<BTreeSet<String>> = std::sync::Mutex::new(BTreeSet::new());
    FETCHED.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn archive_started() -> u32 {
    static STARTED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *STARTED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as u32)
    })
}
