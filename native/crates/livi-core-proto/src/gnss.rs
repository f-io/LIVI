// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Constellation {
    Beidou,
    Galileo,
    Glonass,
    Gps,
    Qzss,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FixQuality {
    #[default]
    None,
    Gps,
    Dgps,
    Pps,
    Rtk,
    RtkFloat,
    Estimated,
    Manual,
    Simulated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum FixMode {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "3d")]
    ThreeD,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Satellite {
    #[serde(serialize_with = "number")]
    pub id: f64,
    pub constellation: Constellation,
    pub used: bool,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub elevation: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub azimuth: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub snr: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Version {
    pub software: String,
    pub hardware: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AntennaStatus {
    Init,
    Unknown,
    Ok,
    Short,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AntennaPower {
    Off,
    On,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Jamming {
    Unknown,
    Ok,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rf {
    pub jamming: Jamming,
    pub antenna_status: AntennaStatus,
    pub antenna_power: AntennaPower,
    pub noise: u16,
    pub agc: u16,
    pub jamming_indicator: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GnssInfo {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baud_rate: Option<u32>,
    pub fix_quality: FixQuality,
    pub fix_mode: FixMode,
    #[serde(serialize_with = "number")]
    pub satellites_used: f64,
    pub satellites_visible: usize,
    pub satellites: Vec<Satellite>,
    pub constellations: Vec<Constellation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rf: Option<Rf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub pdop: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub hdop: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub vdop: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub receiver_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpsFix {
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub lat: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub lng: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub alt: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub heading: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub speed_ms: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub accuracy_m: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub satellites: Option<f64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_number"
    )]
    pub fix_ts: Option<f64>,
}

impl GpsFix {
    pub fn merge(&mut self, newer: &GpsFix) {
        let keep = |old: &mut Option<f64>, new: Option<f64>| {
            if new.is_some() {
                *old = new;
            }
        };
        keep(&mut self.lat, newer.lat);
        keep(&mut self.lng, newer.lng);
        keep(&mut self.alt, newer.alt);
        keep(&mut self.heading, newer.heading);
        keep(&mut self.speed_ms, newer.speed_ms);
        keep(&mut self.accuracy_m, newer.accuracy_m);
        keep(&mut self.satellites, newer.satellites);
        keep(&mut self.fix_ts, newer.fix_ts);
    }
}

fn number<S: Serializer>(value: &f64, s: S) -> Result<S::Ok, S::Error> {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        s.serialize_i64(*value as i64)
    } else {
        s.serialize_f64(*value)
    }
}

fn optional_number<S: Serializer>(value: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(v) => number(v, s),
        None => s.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_info_keeps_the_shape_the_settings_page_reads() {
        let info = GnssInfo {
            connected: true,
            device: Some("/dev/ttyAMA0".into()),
            baud_rate: Some(38400),
            fix_quality: FixQuality::RtkFloat,
            fix_mode: FixMode::ThreeD,
            satellites_used: 8.0,
            satellites_visible: 1,
            satellites: vec![Satellite {
                id: 4.0,
                constellation: Constellation::Gps,
                used: true,
                elevation: Some(70.0),
                azimuth: None,
                snr: Some(45.5),
            }],
            constellations: vec![Constellation::Beidou, Constellation::Gps],
            hdop: Some(0.9),
            receiver_time: Some(1_774_269_319_000.0),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({
                "connected": true,
                "device": "/dev/ttyAMA0",
                "baudRate": 38400,
                "fixQuality": "rtkFloat",
                "fixMode": "3d",
                "satellitesUsed": 8,
                "satellitesVisible": 1,
                "satellites": [
                    { "id": 4, "constellation": "gps", "used": true, "elevation": 70, "snr": 45.5 }
                ],
                "constellations": ["beidou", "gps"],
                "hdop": 0.9,
                "receiverTime": 1_774_269_319_000_u64
            })
        );
        let empty = concat!(
            r#"{"connected":false,"fixQuality":"none","fixMode":"none","satellitesUsed":0,"#,
            r#""satellitesVisible":0,"satellites":[],"constellations":[]}"#
        );
        assert_eq!(serde_json::to_string(&GnssInfo::default()).unwrap(), empty);
    }

    #[test]
    fn a_newer_fix_replaces_only_what_it_says() {
        let mut fix = GpsFix {
            lat: Some(1.0),
            lng: Some(2.0),
            ..Default::default()
        };
        fix.merge(&GpsFix {
            alt: Some(500.0),
            lng: Some(3.0),
            ..Default::default()
        });
        assert_eq!(
            serde_json::to_value(&fix).unwrap(),
            json!({ "lat": 1, "lng": 3, "alt": 500 })
        );
        let odd = GpsFix {
            accuracy_m: Some(f64::NAN),
            speed_ms: Some(-0.0),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&odd).unwrap(),
            r#"{"speedMs":0,"accuracyM":null}"#
        );
    }
}
