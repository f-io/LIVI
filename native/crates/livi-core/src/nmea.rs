// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{SystemTime, UNIX_EPOCH};

pub struct Position {
    pub lat: f64,
    pub lng: f64,
    pub alt: Option<f64>,
    pub heading: Option<f64>,
    pub speed_ms: Option<f64>,
    pub fix_ms: Option<f64>,
    pub accuracy_m: Option<f64>,
}

const KNOTS_PER_MS: f64 = 1.943_844_49;

fn checksum(body: &str) -> String {
    format!("{:02X}", body.bytes().fold(0u8, |cs, b| cs ^ b))
}

fn ddmm(deg: f64, lat: bool) -> (String, char) {
    let abs = deg.abs();
    let whole = abs.floor();
    let minutes = format!("{:07.4}", (abs - whole) * 60.0);
    let ddmm = if lat {
        format!("{:02}{minutes}", whole as u32)
    } else {
        format!("{:03}{minutes}", whole as u32)
    };
    let hemi = match (lat, deg >= 0.0) {
        (true, true) => 'N',
        (true, false) => 'S',
        (false, true) => 'E',
        (false, false) => 'W',
    };
    (ddmm, hemi)
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

pub fn encode(p: &Position) -> String {
    let ms = p
        .fix_ms
        .filter(|t| t.is_finite() && *t != 0.0)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |d| d.as_millis() as f64)
        });
    let secs = (ms / 1000.0).floor() as i64;
    let (days, of_day) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let time = format!(
        "{:02}{:02}{:02}.00",
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60
    );
    let (year, month, day) = civil(days);
    let date = format!("{day:02}{month:02}{:02}", year.rem_euclid(100));

    let (lat, lat_hemi) = ddmm(p.lat, true);
    let (lng, lng_hemi) = ddmm(p.lng, false);
    let hdop = p
        .accuracy_m
        .filter(|a| *a > 0.0)
        .map_or(1.0, |a| (a / 5.0).clamp(0.5, 50.0));
    let alt = p
        .alt
        .map_or_else(|| "0.0".to_string(), |a| format!("{a:.1}"));
    let gga =
        format!("GPGGA,{time},{lat},{lat_hemi},{lng},{lng_hemi},1,08,{hdop:.1},{alt},M,0.0,M,,");
    let speed = p.speed_ms.map_or_else(
        || "0.00".to_string(),
        |s| format!("{:.2}", s * KNOTS_PER_MS),
    );
    let course = p
        .heading
        .map_or_else(|| "0.00".to_string(), |h| format!("{h:.2}"));
    let rmc = format!("GPRMC,{time},A,{lat},{lat_hemi},{lng},{lng_hemi},{speed},{course},{date},,");
    format!("${gga}*{}\r\n${rmc}*{}\r\n", checksum(&gga), checksum(&rmc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lng: f64) -> Position {
        Position {
            lat,
            lng,
            alt: None,
            heading: None,
            speed_ms: None,
            fix_ms: Some(1_759_665_845_000.0),
            accuracy_m: None,
        }
    }

    #[test]
    fn a_fix_becomes_gga_and_rmc() {
        let p = Position {
            alt: Some(34.56),
            heading: Some(271.5),
            speed_ms: Some(13.9),
            accuracy_m: Some(12.0),
            ..at(52.520_008, 13.404_954)
        };
        assert_eq!(
            encode(&p),
            "$GPGGA,120405.00,5231.2005,N,01324.2972,E,1,08,2.4,34.6,M,0.0,M,,*69\r\n\
             $GPRMC,120405.00,A,5231.2005,N,01324.2972,E,27.02,271.50,051025,,*0C\r\n"
        );
    }

    #[test]
    fn the_south_west_and_missing_values_have_their_defaults() {
        assert_eq!(
            encode(&Position {
                accuracy_m: Some(0.5),
                ..at(-33.8688, -151.2093)
            }),
            "$GPGGA,120405.00,3352.1280,S,15112.5580,W,1,08,0.5,0.0,M,0.0,M,,*5E\r\n\
             $GPRMC,120405.00,A,3352.1280,S,15112.5580,W,0.00,0.00,051025,,*3F\r\n"
        );
        assert_eq!(
            encode(&Position {
                accuracy_m: Some(1000.0),
                ..at(1.0, 1.0)
            }),
            "$GPGGA,120405.00,0100.0000,N,00100.0000,E,1,08,50.0,0.0,M,0.0,M,,*63\r\n\
             $GPRMC,120405.00,A,0100.0000,N,00100.0000,E,0.00,0.00,051025,,*32\r\n"
        );
        let now = encode(&Position {
            fix_ms: Some(0.0),
            ..at(1.0, 1.0)
        });
        assert!(now.starts_with("$GPGGA,") && !now.contains("051025"));
    }

    #[test]
    fn days_since_the_epoch_give_the_calendar_date() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_366), (2025, 10, 5));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(-1), (1969, 12, 31));
    }
}
