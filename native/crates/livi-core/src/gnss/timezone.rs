// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;
use std::sync::Mutex;

use super::SET_TIME_HELPER;
use super::tz_data::{PACKED, ZONES};
use crate::privileged::sudo;

const ZONEINFO: &str = "/usr/share/zoneinfo";

static APPLIED: Mutex<Option<String>> = Mutex::new(None);
static GPS_ZONE: Mutex<Option<String>> = Mutex::new(None);

pub fn zone_for_position(lat: f64, lng: f64) -> Option<&'static str> {
    if !lat.is_finite() || !lng.is_finite() || lat.abs() > 90.0 || lng.abs() > 180.0 {
        return None;
    }
    Some(lookup(lat, lng))
}

fn lookup(lat: f64, lng: f64) -> &'static str {
    if lat >= 90.0 {
        return "Etc/GMT";
    }
    let packed = PACKED.as_bytes();
    let at = |i: i64| 56 * i64::from(packed[i as usize]) + i64::from(packed[i as usize + 1]) - 1995;
    let leaves = ZONES.len() as i64;
    let mut x = 48.0 * (180.0 + lng) / 360.000_000_000_000_06;
    let mut y = 24.0 * (90.0 - lat) / 180.000_000_000_000_03;
    let (mut col, mut row) = (x as i64, y as i64);
    let mut node = -1;
    let mut next = at(96 * row + 2 * col);
    while next + leaves < 3136 {
        node += next + 1;
        y = (2.0 * (y - row as f64)) % 2.0;
        row = y as i64;
        x = (2.0 * (x - col as f64)) % 2.0;
        col = x as i64;
        next = at(8 * node + 4 * row + 2 * col + 2304);
    }
    ZONES[(next + leaves - 3136) as usize]
}

pub fn note_gps_zone(zone: &str) {
    *GPS_ZONE.lock().unwrap_or_else(|e| e.into_inner()) = Some(zone.to_string());
}

fn system_zone() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if known(tz) {
            return Some(tz.to_string());
        }
    }
    if let Ok(target) = std::fs::read_link("/etc/localtime")
        && let Some((_, zone)) = target.to_string_lossy().split_once("zoneinfo/")
    {
        return Some(zone.to_string());
    }
    let zone = std::fs::read_to_string("/etc/timezone").ok()?;
    Some(zone.trim().to_string()).filter(|z| !z.is_empty())
}

fn current_zone() -> Option<String> {
    let applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    applied.or_else(system_zone)
}

fn known(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'/' | b'-'))
        && !zone.starts_with('/')
        && Path::new(ZONEINFO).join(zone).is_file()
}

pub async fn apply(zone: String) {
    if current_zone().as_deref() == Some(zone.as_str()) {
        return;
    }
    if !known(&zone) {
        eprintln!("[timezone] {zone} is not a zone this system knows");
        return;
    }
    if !Path::new(SET_TIME_HELPER).exists() {
        eprintln!(
            "[timezone] {SET_TIME_HELPER} missing, run the installer again to let LIVI set the zone"
        );
        return;
    }
    if !sudo(&[SET_TIME_HELPER, "tz", &zone]).await {
        eprintln!("[timezone] could not set the zone to {zone}");
        return;
    }
    println!("[timezone] host zone set to {zone}");
    *APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = Some(zone);
}

fn offset_minutes(zone: &str, at: jiff::Timestamp) -> Option<i32> {
    let tz = jiff::tz::TimeZone::get(zone).ok()?;
    Some(tz.to_offset(at).seconds() / 60)
}

fn observes_dst(zone: &str) -> bool {
    let at = |month| {
        jiff::civil::date(2026, month, 15)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .ok()
    };
    match (at(1), at(7)) {
        (Some(winter), Some(summer)) => {
            let (w, s) = (
                offset_minutes(zone, winter.timestamp()),
                offset_minutes(zone, summer.timestamp()),
            );
            w.is_some() && s.is_some() && w != s
        }
        _ => false,
    }
}

fn zone_for_offset(minutes: i32, now: jiff::Timestamp) -> Option<String> {
    if minutes.abs() > 14 * 60 {
        return None;
    }
    if minutes % 60 == 0 {
        let hours = minutes / 60;
        return Some(match hours {
            0 => "UTC".to_string(),
            h if h > 0 => format!("Etc/GMT-{h}"),
            h => format!("Etc/GMT+{}", -h),
        });
    }
    let mut zones: Vec<String> = jiff::tz::db()
        .available()
        .map(|name| name.as_str().to_string())
        .filter(|name| {
            name.contains('/')
                && !name.starts_with("Etc/")
                && !name.starts_with("posix/")
                && !name.starts_with("right/")
        })
        .collect();
    zones.sort();
    let mut fallback = None;
    for zone in zones {
        if offset_minutes(&zone, now) != Some(minutes) {
            continue;
        }
        if !observes_dst(&zone) {
            return Some(zone);
        }
        fallback.get_or_insert(zone);
    }
    fallback
}

pub async fn apply_phone_offset(minutes: i32) {
    if !cfg!(target_os = "linux") || GPS_ZONE.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return;
    }
    let now = jiff::Timestamp::now();
    if current_zone().and_then(|z| offset_minutes(&z, now)) == Some(minutes) {
        return;
    }
    if let Some(zone) = zone_for_offset(minutes, now) {
        apply(zone).await;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn positions_name_the_zones_tz_lookup_named() {
        let vectors: Vec<(f64, f64, String)> =
            serde_json::from_str::<Vec<Value>>(include_str!("tz_vectors.json"))
                .unwrap()
                .into_iter()
                .map(|v| {
                    (
                        v[0].as_f64().unwrap(),
                        v[1].as_f64().unwrap(),
                        v[2].as_str().unwrap().into(),
                    )
                })
                .collect();
        assert!(vectors.len() > 600);
        for (lat, lng, zone) in vectors {
            assert_eq!(
                zone_for_position(lat, lng),
                Some(zone.as_str()),
                "{lat},{lng}"
            );
        }
    }

    #[test]
    fn the_known_places() {
        for (zone, lat, lng) in [
            ("Europe/Berlin", 53.3536, 10.5633),
            ("Europe/Berlin", 52.52, 13.405),
            ("Asia/Kolkata", 22.5726, 88.3639),
            ("Asia/Kathmandu", 27.7172, 85.324),
            ("Asia/Yangon", 16.8409, 96.1735),
            ("America/St_Johns", 47.5615, -52.7126),
            ("Pacific/Chatham", -43.95, -176.55),
        ] {
            assert_eq!(zone_for_position(lat, lng), Some(zone));
        }
        assert!(
            zone_for_position(30.0, -40.0)
                .unwrap()
                .starts_with("Etc/GMT")
        );
        assert_eq!(zone_for_position(90.0, 0.0), Some("Etc/GMT"));
    }

    #[test]
    fn positions_off_the_globe_have_no_zone() {
        assert_eq!(zone_for_position(91.0, 0.0), None);
        assert_eq!(zone_for_position(0.0, 181.0), None);
        assert_eq!(zone_for_position(f64::NAN, 0.0), None);
        assert_eq!(zone_for_position(0.0, f64::INFINITY), None);
        assert_eq!(zone_for_position(999.0, 999.0), None);
    }

    #[test]
    fn a_phone_offset_finds_a_zone() {
        let now = jiff::Timestamp::from_second(1_790_000_000).unwrap();
        assert_eq!(zone_for_offset(0, now).as_deref(), Some("UTC"));
        assert_eq!(zone_for_offset(120, now).as_deref(), Some("Etc/GMT-2"));
        assert_eq!(zone_for_offset(-300, now).as_deref(), Some("Etc/GMT+5"));
        assert_eq!(zone_for_offset(15 * 60, now), None);
        let india = zone_for_offset(330, now).unwrap();
        assert_eq!(offset_minutes(&india, now), Some(330));
        assert!(!observes_dst(&india));
        assert!(observes_dst("Europe/Berlin"));
        assert_eq!(offset_minutes("Middle/Earth", now), None);
    }

    #[test]
    fn only_zone_names_reach_the_helper() {
        assert!(!known(""));
        assert!(!known("Middle/Earth"));
        assert!(!known("../../etc/passwd"));
        assert!(!known("/etc/passwd"));
        assert!(!known("Europe/Berlin; reboot"));
    }
}
