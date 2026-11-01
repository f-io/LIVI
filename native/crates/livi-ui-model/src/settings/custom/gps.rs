// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

use super::{info, mono};
use crate::i18n::Strings;
use crate::settings::view::Row;

const CONSTELLATIONS: [(&str, &str); 6] = [
    ("gps", "GPS"),
    ("glonass", "GLONASS"),
    ("galileo", "Galileo"),
    ("beidou", "BeiDou"),
    ("qzss", "QZSS"),
    ("unknown", "?"),
];

const FIX_SATELLITES: usize = 4;

fn fix_quality(t: &Strings, q: &str) -> String {
    let (key, text) = match q {
        "none" => ("settings.gpsFixNone", "No fix"),
        "gps" => ("settings.gpsFixGps", "GPS"),
        "dgps" => ("settings.gpsFixDgps", "DGPS"),
        "pps" => ("settings.gpsFixPps", "PPS"),
        "rtk" => ("settings.gpsFixRtk", "RTK fixed"),
        "rtkFloat" => ("settings.gpsFixRtkFloat", "RTK float"),
        "estimated" => ("settings.gpsFixEstimated", "Estimated"),
        "manual" => ("settings.gpsFixManual", "Manual"),
        "simulated" => ("settings.gpsFixSimulated", "Simulated"),
        _ => return String::new(),
    };
    t.t(key, text)
}

fn rating(t: &Strings, level: f64) -> String {
    if level >= 44.0 {
        t.t("settings.gpsRatingExcellent", "excellent")
    } else if level >= 38.0 {
        t.t("settings.gpsRatingGood", "good")
    } else if level >= 32.0 {
        t.t("settings.gpsRatingWeak", "weak")
    } else {
        t.t("settings.gpsRatingPoor", "poor")
    }
}

fn signal_level(sats: &[Value]) -> Option<f64> {
    let mut snrs: Vec<f64> = sats.iter().filter_map(|s| s["snr"].as_f64()).collect();
    if snrs.is_empty() {
        return None;
    }
    snrs.sort_by(|a, b| b.total_cmp(a));
    let top = &snrs[..snrs.len().min(FIX_SATELLITES)];
    Some(top.iter().sum::<f64>() / top.len() as f64)
}

pub fn utc_time(ms: f64) -> String {
    let secs = (ms / 1000.0).floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn info_rows(state: &Value, t: &Strings) -> Option<Vec<Row>> {
    let gnss = state["telemetry"].get("gnss").filter(|g| g.is_object())?;
    let fix = &state["telemetry"]["gps"];
    let mut out = vec![info(
        t.t("settings.gpsStatus", "Status"),
        None,
        if gnss["connected"].as_bool() == Some(true) {
            t.t("settings.gpsConnected", "Connected")
        } else {
            gnss["error"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| t.t("settings.gpsDisconnected", "Not connected"))
        },
    )];
    if let Some(ms) = gnss["receiverTime"].as_f64() {
        out.push(mono(
            t.t("settings.gpsReceiverTime", "Receiver Time"),
            utc_time(ms),
        ));
    }
    if let Some(tz) = gnss["timezone"].as_str().filter(|s| !s.is_empty()) {
        out.push(mono(t.t("settings.gpsTimezone", "Time Zone"), tz.into()));
    }
    out.push(info(
        t.t("settings.gpsFixQuality", "Fix Quality"),
        None,
        fix_quality(t, gnss["fixQuality"].as_str().unwrap_or("none")),
    ));
    let mode = gnss["fixMode"].as_str().unwrap_or("none");
    out.push(info(
        t.t("settings.gpsFixMode", "Fix Mode"),
        None,
        if mode == "none" {
            t.t("settings.gpsFixNone", "No fix")
        } else {
            mode.to_uppercase()
        },
    ));
    out.push(mono(
        t.t("settings.gpsSatellites", "Satellites (used / visible)"),
        format!(
            "{} / {}",
            gnss["satellitesUsed"].as_f64().unwrap_or(0.0),
            gnss["satellitesVisible"].as_u64().unwrap_or(0)
        ),
    ));
    let sats = gnss["satellites"].as_array().cloned().unwrap_or_default();
    let reported: Vec<&str> = gnss["constellations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    for (key, name) in CONSTELLATIONS {
        let of: Vec<&Value> = sats
            .iter()
            .filter(|s| s["constellation"].as_str() == Some(key))
            .collect();
        if of.is_empty() && !reported.contains(&key) {
            continue;
        }
        let used = of
            .iter()
            .filter(|s| s["used"].as_bool() == Some(true))
            .count();
        out.push(mono(name.into(), format!("{used} / {}", of.len())));
    }
    if let Some(level) = signal_level(&sats) {
        out.push(mono(
            t.t("settings.gpsSignal", "Signal"),
            format!("{level:.0} dB-Hz · {}", rating(t, level)),
        ));
    }
    if gnss["hdop"].as_f64().is_some_and(|h| h < 50.0) {
        let dop: Vec<String> = ["pdop", "hdop", "vdop"]
            .iter()
            .map(|k| gnss[*k].as_f64().map_or("–".into(), |v| format!("{v:.1}")))
            .collect();
        out.push(mono(t.t("settings.gpsDop", "DOP (P/H/V)"), dop.join(" / ")));
    }
    if let Some(a) = fix["accuracyM"].as_f64() {
        out.push(mono(
            t.t("settings.gpsAccuracy", "Accuracy"),
            format!("± {a:.1} m"),
        ));
    }
    if let (Some(lat), Some(lng)) = (fix["lat"].as_f64(), fix["lng"].as_f64()) {
        out.push(mono(
            t.t("settings.gpsPosition", "Position"),
            format!("{lat:.6}, {lng:.6}"),
        ));
    }
    if let Some(alt) = fix["alt"].as_f64() {
        out.push(mono(
            t.t("settings.gpsAltitude", "Altitude"),
            format!("{alt:.1} m"),
        ));
    }
    if let Some(ms) = fix["speedMs"].as_f64() {
        let mph = state["config"]["speedUnit"].as_str() == Some("mph");
        let kph = ms * 3.6;
        let (v, unit) = if mph {
            (kph * 0.621_371, "mph")
        } else {
            (kph, "km/h")
        };
        out.push(mono(
            t.t("settings.gpsSpeed", "Speed"),
            format!("{v:.1} {unit}"),
        ));
    }
    Some(out)
}

pub fn data(state: &Value, t: &Strings) -> Vec<Row> {
    info_rows(state, t).unwrap_or_else(|| {
        vec![super::info(
            t.t("settings.gpsStatus", "Status"),
            None,
            t.t("settings.gpsDisabled", "Disabled"),
        )]
    })
}

fn word(t: &Strings, group: &str, v: &str) -> String {
    let (key, text) = match (group, v) {
        ("antenna", "init") => ("settings.gpsAntennaInit", "Initialising"),
        ("antenna", "ok") => ("settings.gpsAntennaOk", "OK"),
        ("antenna", "short") => ("settings.gpsAntennaShort", "Short circuit"),
        ("antenna", "open") => ("settings.gpsAntennaOpen", "Open (no current)"),
        ("power", "off") => ("settings.gpsAntennaPowerOff", "Off"),
        ("power", "on") => ("settings.gpsAntennaPowerOn", "On"),
        ("jamming", "ok") => ("settings.gpsJammingOk", "None"),
        ("jamming", "warning") => ("settings.gpsJammingWarning", "Warning"),
        ("jamming", "critical") => ("settings.gpsJammingCritical", "Critical"),
        _ => ("settings.gpsAntennaUnknown", "Unknown"),
    };
    t.t(key, text)
}

pub fn hardware(state: &Value, t: &Strings) -> Vec<Row> {
    let gnss = &state["telemetry"]["gnss"];
    let v = &gnss["version"];
    let mut out = Vec::new();
    if let Some(m) = v["model"].as_str() {
        out.push(super::info(
            t.t("settings.gpsModel", "Model"),
            None,
            m.into(),
        ));
    }
    for (field, key, text) in [
        ("firmware", "settings.gpsFirmware", "Firmware"),
        ("protocol", "settings.gpsProtocol", "Protocol Version"),
        ("software", "settings.gpsSoftware", "Software"),
        ("hardware", "settings.gpsHardware", "Hardware"),
    ] {
        if let Some(s) = v[field].as_str().filter(|s| !s.is_empty()) {
            out.push(mono(t.t(key, text), s.into()));
        }
    }
    let supported: Vec<&str> = v["supported"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !supported.is_empty() {
        out.push(super::info(
            t.t("settings.gpsSupported", "Supported Systems"),
            None,
            supported.join(", "),
        ));
    }
    let rf = &gnss["rf"];
    if rf.is_object() {
        out.push(super::info(
            t.t("settings.gpsAntennaStatus", "Antenna"),
            None,
            word(t, "antenna", rf["antennaStatus"].as_str().unwrap_or("")),
        ));
        out.push(super::info(
            t.t("settings.gpsAntennaPower", "Antenna Power"),
            None,
            word(t, "power", rf["antennaPower"].as_str().unwrap_or("")),
        ));
        out.push(mono(
            t.t("settings.gpsJamming", "Interference"),
            format!(
                "{} · {}",
                word(t, "jamming", rf["jamming"].as_str().unwrap_or("")),
                rf["jammingIndicator"].as_u64().unwrap_or(0)
            ),
        ));
        out.push(mono(
            t.t("settings.gpsAgc", "AGC"),
            rf["agc"].as_u64().unwrap_or(0).to_string(),
        ));
        out.push(mono(
            t.t("settings.gpsNoise", "Noise"),
            rf["noise"].as_u64().unwrap_or(0).to_string(),
        ));
    }
    if out.is_empty() {
        out.push(super::info(
            t.t("settings.gpsStatus", "Status"),
            None,
            t.t("settings.gpsNoHwInfo", "No hardware data yet"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn receiver_time_is_utc() {
        assert_eq!(utc_time(1_760_000_000_000.0), "2025-10-09 08:53:20 UTC");
    }

    #[test]
    fn constellations_count_used_over_visible() {
        let s = json!({ "telemetry": { "gnss": {
            "connected": true, "fixQuality": "gps", "fixMode": "3d",
            "satellitesUsed": 1, "satellitesVisible": 2,
            "constellations": ["gps", "galileo"],
            "satellites": [
                { "id": 1, "constellation": "gps", "used": true, "snr": 40 },
                { "id": 2, "constellation": "gps", "used": false, "snr": 30 }
            ] } } });
        let rows = data(&s, &Strings::new("en"));
        let find = |l: &str| rows.iter().find(|r| r.label == l).map(|r| r.value.clone());
        assert_eq!(find("GPS").as_deref(), Some("1 / 2"));
        assert_eq!(find("Galileo").as_deref(), Some("0 / 0"));
        assert_eq!(find("Fix Mode").as_deref(), Some("3D"));
        assert_eq!(find("Signal").as_deref(), Some("35 dB-Hz · weak"));
    }

    #[test]
    fn no_receiver_says_disabled() {
        let rows = data(&json!({}), &Strings::new("en"));
        assert_eq!(rows[0].value, "Disabled");
    }
}
