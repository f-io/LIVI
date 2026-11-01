// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::info::{Constellation, FixMode, FixQuality, GpsFix, Satellite};
use super::{latin1, trim};

const KNOTS_TO_MS: f64 = 0.514444;
const BUFFER_MAX: usize = 16384;
const BUFFER_KEEP: usize = 4096;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Update {
    pub gps: Option<GpsFix>,
    pub fix_quality: Option<FixQuality>,
    pub fix_mode: Option<FixMode>,
    pub pdop: Option<f64>,
    pub hdop: Option<f64>,
    pub vdop: Option<f64>,
    pub receiver_time: Option<f64>,
    pub satellites: Option<Vec<Satellite>>,
    pub satellites_used: Option<f64>,
}

fn constellation(talker: &str) -> Constellation {
    match talker {
        "GP" => Constellation::Gps,
        "GL" => Constellation::Glonass,
        "GA" => Constellation::Galileo,
        "GB" | "BD" => Constellation::Beidou,
        "GQ" => Constellation::Qzss,
        _ => Constellation::Unknown,
    }
}

fn fix_quality(code: f64) -> FixQuality {
    match code {
        1.0 => FixQuality::Gps,
        2.0 => FixQuality::Dgps,
        3.0 => FixQuality::Pps,
        4.0 => FixQuality::Rtk,
        5.0 => FixQuality::RtkFloat,
        6.0 => FixQuality::Estimated,
        7.0 => FixQuality::Manual,
        8.0 => FixQuality::Simulated,
        _ => FixQuality::None,
    }
}

fn body(sentence: &str, star: Option<usize>) -> &str {
    let start = usize::from(sentence.starts_with('$'));
    &sentence[start..star.unwrap_or(sentence.len()).max(start)]
}

pub fn checksum_valid(sentence: &str) -> bool {
    let Some(star) = sentence.rfind('*') else {
        return true;
    };
    let want: String = sentence[star + 1..].chars().take(2).collect();
    if want.len() != 2 || !want.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let sum = body(sentence, Some(star))
        .chars()
        .fold(0u32, |sum, c| sum ^ u32::from(c));
    u32::from_str_radix(&want, 16) == Ok(sum)
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    pending_sweep: HashMap<String, Vec<Satellite>>,
    visible: BTreeMap<Constellation, Vec<Satellite>>,
    used_ids: Vec<f64>,
    seen: BTreeSet<Constellation>,
    last_date: String,
}

impl Decoder {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Update> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > BUFFER_MAX {
            self.buffer.drain(..self.buffer.len() - BUFFER_KEEP);
        }
        let mut out = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&b| b == b'\r' || b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = latin1(&line[..end]);
            let line = trim(&line);
            if line.starts_with('$')
                && let Some(update) = self.decode(line)
            {
                out.push(update);
            }
        }
        out
    }

    pub fn decode(&mut self, sentence: &str) -> Option<Update> {
        if !checksum_valid(sentence) {
            return None;
        }
        let body = body(sentence, sentence.rfind('*'));
        let fields: Vec<&str> = body.split(',').collect();
        let header = fields[0];
        if header.chars().count() < 5 {
            return None;
        }
        let split = header
            .char_indices()
            .nth(2)
            .map_or(header.len(), |(i, _)| i);
        let (talker, kind) = header.split_at(split);
        let f = |i: usize| fields.get(i).copied();
        match kind {
            "GGA" => Some(gga(f)),
            "RMC" => Some(self.rmc(f)),
            "GSA" => Some(self.gsa(f)),
            "GSV" => self.gsv(talker, f),
            "GST" => gst(f),
            _ => None,
        }
    }

    pub fn constellations_seen(&self) -> Vec<Constellation> {
        self.seen.iter().copied().collect()
    }

    pub fn satellites_in_view(&self) -> Vec<Satellite> {
        let mut all = Vec::new();
        for list in self.visible.values() {
            let mut sats: Vec<Satellite> = list
                .iter()
                .map(|s| Satellite {
                    used: self.used_ids.contains(&s.id),
                    ..s.clone()
                })
                .collect();
            sats.sort_by(|a, b| a.id.partial_cmp(&b.id).unwrap_or(std::cmp::Ordering::Equal));
            all.extend(sats);
        }
        all
    }

    fn rmc<'a>(&mut self, f: impl Fn(usize) -> Option<&'a str>) -> Update {
        let active = f(2) == Some("A");
        if let Some(date) = f(9).filter(|d| !d.is_empty()) {
            self.last_date = date.to_string();
        }
        let time = utc_ms(f(1), &self.last_date);
        let mut update = Update {
            receiver_time: time,
            ..Default::default()
        };
        if !active {
            return update;
        }
        let (Some(lat), Some(lng)) = (coord(f(3), f(4)), coord(f(5), f(6))) else {
            return update;
        };
        let knots = num(f(7));
        let mut gps = GpsFix {
            lat: Some(lat),
            lng: Some(lng),
            speed_ms: knots.map(|k| k * KNOTS_TO_MS),
            fix_ts: time,
            ..Default::default()
        };
        if let (Some(course), Some(knots)) = (num(f(8)), knots)
            && knots > 0.5
        {
            gps.heading = Some(course);
        }
        update.gps = Some(gps);
        update
    }

    fn gsa<'a>(&mut self, f: impl Fn(usize) -> Option<&'a str>) -> Update {
        let fix_mode = match num(f(2)) {
            Some(3.0) => FixMode::ThreeD,
            Some(2.0) => FixMode::TwoD,
            _ => FixMode::None,
        };
        for id in (3..=14).filter_map(|i| num(f(i))) {
            if !self.used_ids.contains(&id) {
                self.used_ids.push(id);
            }
        }
        Update {
            fix_mode: Some(fix_mode),
            pdop: num(f(15)),
            hdop: num(f(16)),
            vdop: num(f(17)),
            satellites: Some(self.satellites_in_view()),
            ..Default::default()
        }
    }

    fn gsv<'a>(&mut self, talker: &str, f: impl Fn(usize) -> Option<&'a str>) -> Option<Update> {
        let (total, index) = (num(f(1))?, num(f(2))?);
        let constellation = constellation(talker);
        self.seen.insert(constellation);
        let pending = self.pending_sweep.remove(talker).unwrap_or_default();
        let mut sweep = if index == 1.0 { Vec::new() } else { pending };

        let mut i = 4;
        while f(i + 2).is_some() {
            if let Some(id) = num(f(i)) {
                let sat = Satellite {
                    id,
                    constellation,
                    used: false,
                    elevation: num(f(i + 1)),
                    azimuth: num(f(i + 2)),
                    snr: num(f(i + 3)),
                };
                match sweep.iter().position(|s| s.id == id) {
                    None => sweep.push(sat),
                    Some(k) if sat.snr.unwrap_or(-1.0) > sweep[k].snr.unwrap_or(-1.0) => {
                        sweep[k] = sat;
                    }
                    Some(_) => {}
                }
            }
            i += 4;
        }

        if index < total {
            self.pending_sweep.insert(talker.to_string(), sweep);
            return None;
        }
        if sweep.is_empty() {
            self.visible.remove(&constellation);
        } else {
            self.visible.insert(constellation, sweep);
        }
        Some(Update {
            satellites: Some(self.satellites_in_view()),
            ..Default::default()
        })
    }
}

fn gga<'a>(f: impl Fn(usize) -> Option<&'a str>) -> Update {
    let quality = num(f(6)).map_or(FixQuality::None, fix_quality);
    let used = num(f(7));
    let mut update = Update {
        fix_quality: Some(quality),
        satellites_used: used,
        hdop: num(f(8)),
        ..Default::default()
    };
    if quality != FixQuality::None
        && let (Some(lat), Some(lng)) = (coord(f(2), f(3)), coord(f(4), f(5)))
    {
        update.gps = Some(GpsFix {
            lat: Some(lat),
            lng: Some(lng),
            alt: num(f(9)),
            satellites: used,
            ..Default::default()
        });
    }
    update
}

fn gst<'a>(f: impl Fn(usize) -> Option<&'a str>) -> Option<Update> {
    let (lat_error, lng_error) = (num(f(6))?, num(f(7))?);
    let gps = GpsFix {
        accuracy_m: Some(lat_error.hypot(lng_error)),
        ..Default::default()
    };
    Some(Update {
        gps: Some(gps),
        ..Default::default()
    })
}

fn number(text: &str) -> Option<f64> {
    let t = trim(text);
    if t.is_empty() {
        return Some(0.0);
    }
    let radix = |prefix: [&str; 2], radix: u32| {
        let digits = t
            .strip_prefix(prefix[0])
            .or_else(|| t.strip_prefix(prefix[1]))?;
        if digits.is_empty() {
            return Some(None);
        }
        Some(digits.chars().try_fold(0f64, |v, c| {
            c.to_digit(radix)
                .map(|d| v * f64::from(radix) + f64::from(d))
        }))
    };
    if let Some(value) = radix(["0x", "0X"], 16)
        .or_else(|| radix(["0o", "0O"], 8))
        .or_else(|| radix(["0b", "0B"], 2))
    {
        return value;
    }
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if unsigned == "Infinity" {
        return Some(if t.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if !unsigned
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return None;
    }
    t.parse().ok()
}

fn num(field: Option<&str>) -> Option<f64> {
    number(field.filter(|f| !f.is_empty())?).filter(|n| n.is_finite())
}

fn coord(value: Option<&str>, hemisphere: Option<&str>) -> Option<f64> {
    let (value, hemisphere) = (
        value.filter(|v| !v.is_empty())?,
        hemisphere.filter(|h| !h.is_empty())?,
    );
    let raw = number(value).filter(|n| n.is_finite())?.abs();
    let degrees = (raw / 100.0).floor();
    let decimal = degrees + (raw - degrees * 100.0) / 60.0;
    Some(if matches!(hemisphere, "S" | "W") {
        -decimal
    } else {
        decimal
    })
}

fn chars(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

fn utc_ms(time: Option<&str>, date: &str) -> Option<f64> {
    let time = time?;
    let time_len = time.chars().count();
    if time_len < 6 || date.chars().count() < 6 {
        return None;
    }
    let part = |s: &str, from, to| number(&chars(s, from, to)).filter(|n| n.is_finite());
    let hours = part(time, 0, 2)?;
    let minutes = part(time, 2, 4)?;
    let seconds = part(time, 4, time_len)?;
    let day = part(date, 0, 2)?;
    let month = part(date, 2, 4)?;
    let year = part(date, 4, 6)?;
    let millis = round_half_up((seconds % 1.0) * 1000.0);
    Some(date_utc(
        2000.0 + year,
        month - 1.0,
        day,
        hours,
        minutes,
        seconds.floor(),
        millis,
    ))
}

pub(super) fn round_half_up(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn days_to_month(year: i64, month: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn date_utc(
    year: f64,
    month: f64,
    day: f64,
    hours: f64,
    minutes: f64,
    seconds: f64,
    millis: f64,
) -> f64 {
    let (year, month) = (year.trunc(), month.trunc());
    let year = year + (month / 12.0).floor();
    let month = month.rem_euclid(12.0);
    let days = days_to_month(year as i64, month as i64 + 1) as f64 + day.trunc() - 1.0;
    let time = hours.trunc() * 3_600_000.0
        + minutes.trunc() * 60_000.0
        + seconds.trunc() * 1000.0
        + millis.trunc();
    let t = days * 86_400_000.0 + time;
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    t.trunc() + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLD: [&str; 6] = [
        "$GNRMC,,V,,,,,,,,,,N,V*37",
        "$GNVTG,,,,,,,,,N*2E",
        "$GNGGA,,,,,,0,00,99.99,,,,,,*56",
        "$GNGSA,A,1,,,,,,,,,,,,,99.99,99.99,99.99,1*33",
        "$GPGSV,1,1,00,1*64",
        "$GLGSV,1,1,00,1*78",
    ];
    const GGA_FIX: &str = "$GPGGA,123519.00,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*69";
    const RMC_FIX: &str = "$GPRMC,123519.00,A,4807.038,N,01131.000,E,022.4,084.4,230326,,,A*5B";
    const RMC_TIME: f64 = 1_774_269_319_000.0;

    fn one(d: &mut Decoder, line: &str) -> Update {
        let mut updates = d.push(format!("{line}\r\n").as_bytes());
        assert_eq!(updates.len(), 1, "{line}");
        updates.remove(0)
    }

    fn close(a: Option<f64>, b: f64, digits: i32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 10f64.powi(-digits) / 2.0)
    }

    #[test]
    fn the_checksum_trailer_is_checked() {
        assert!(checksum_valid("$GNRMC,,V,,,,,,,,,,N,V*37"));
        assert!(!checksum_valid("$GNRMC,,V,,,,,,,,,,N,V*38"));
        assert!(!checksum_valid("$GPGGA,1*ZZ"));
        assert!(checksum_valid("$GPGGA,123519.00"));
        assert!(checksum_valid("$GPGST,123519.00,1.5,,,,3.0,4.0,2.0*750"));
    }

    #[test]
    fn sentences_are_framed_from_raw_reads() {
        let mut d = Decoder::default();
        assert!(
            !d.push(format!("noise\r\n{}\r\n", COLD.join("\r\n")).as_bytes())
                .is_empty()
        );

        let mut d = Decoder::default();
        assert!(d.push(b"$GPGGA,123519.00,4807.038,N,0113").is_empty());
        let updates = d.push(b"1.000,E,1,08,0.9,545.4,M,46.9,M,,*69\r\n");
        assert!(close(
            updates[0].gps.as_ref().and_then(|g| g.lat),
            48.1173,
            4
        ));

        let broken = "$GPGGA,123519.00,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*00\r\n";
        assert!(Decoder::default().push(broken.as_bytes()).is_empty());

        let mut d = Decoder::default();
        d.push("x".repeat(20000).as_bytes());
        assert!(d.buffer.len() <= BUFFER_KEEP);
        assert_eq!(
            one(&mut d, &format!("\r\n{GGA_FIX}")).fix_quality,
            Some(FixQuality::Gps)
        );

        assert!(
            Decoder::default()
                .push(b"$GPZDA,123519.00,23,03,2026,00,00*6F\r\n")
                .is_empty()
        );
        assert!(Decoder::default().push(b"$GP\r\n").is_empty());
        assert_eq!(
            Decoder::default()
                .decode(&GGA_FIX[1..])
                .and_then(|u| u.fix_quality),
            Some(FixQuality::Gps)
        );
    }

    #[test]
    fn gga_gives_position_altitude_and_quality() {
        let u = one(&mut Decoder::default(), GGA_FIX);
        assert_eq!(u.fix_quality, Some(FixQuality::Gps));
        assert_eq!((u.satellites_used, u.hdop), (Some(8.0), Some(0.9)));
        let gps = u.gps.unwrap();
        assert!(close(gps.lat, 48.1173, 4) && close(gps.lng, 11.51667, 4));
        assert_eq!((gps.alt, gps.satellites), (Some(545.4), Some(8.0)));

        let u = one(&mut Decoder::default(), "$GNGGA,,,,,,0,00,99.99,,,,,,*56");
        assert_eq!(
            (u.fix_quality, u.gps, u.satellites_used),
            (Some(FixQuality::None), None, Some(0.0))
        );

        let south_west = "$GPGGA,123519.00,3352.000,S,15112.000,W,1,06,1.2,10.0,M,0.0,M,,*6B";
        let gps = one(&mut Decoder::default(), south_west).gps.unwrap();
        assert!(gps.lat.unwrap() < 0.0 && gps.lng.unwrap() < 0.0);

        let unknown = "$GPGGA,123519.00,4807.038,N,01131.000,E,9,08,0.9,545.4,M,,,,*39";
        assert_eq!(
            one(&mut Decoder::default(), unknown).fix_quality,
            Some(FixQuality::None)
        );
    }

    #[test]
    fn rmc_gives_speed_course_and_the_receiver_clock() {
        let gps = one(&mut Decoder::default(), RMC_FIX).gps.unwrap();
        assert!(close(gps.speed_ms, 22.4 * 0.514444, 3));
        assert!(close(gps.heading, 84.4, 1));
        assert_eq!(gps.fix_ts, Some(RMC_TIME));

        let still = "$GPRMC,123519.00,A,4807.038,N,01131.000,E,000.1,084.4,230326,,,A*5E";
        let gps = one(&mut Decoder::default(), still).gps.unwrap();
        assert_eq!(gps.heading, None);
        assert!(gps.speed_ms.is_some());

        let void = one(
            &mut Decoder::default(),
            "$GPRMC,123519.00,V,,,,,,,230326,,,N*76",
        );
        assert_eq!((void.gps, void.receiver_time), (None, Some(RMC_TIME)));

        let mut d = Decoder::default();
        one(&mut d, RMC_FIX);
        let later = "$GPRMC,123520.00,A,4807.038,N,01131.000,E,022.4,084.4,,,,A*57";
        assert_eq!(one(&mut d, later).receiver_time, Some(RMC_TIME + 1000.0));

        let undated = "$GPRMC,123519.00,A,4807.038,N,01131.000,E,022.4,084.4,,,,A*5D";
        let u = one(&mut Decoder::default(), undated);
        assert_eq!(u.receiver_time, None);
        assert!(close(u.gps.unwrap().lat, 48.1173, 4));

        let u = one(&mut Decoder::default(), "$GNRMC,,V,,,,,,,,,,N,V*37");
        assert_eq!((u.gps, u.receiver_time), (None, None));
    }

    #[test]
    fn gsa_gives_the_mode_the_dops_and_the_satellites_in_use() {
        let u = one(
            &mut Decoder::default(),
            "$GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1*39",
        );
        assert_eq!(u.fix_mode, Some(FixMode::ThreeD));
        assert_eq!((u.pdop, u.hdop, u.vdop), (Some(2.5), Some(1.3), Some(2.1)));

        let mut d = Decoder::default();
        let two = one(&mut d, "$GPGSA,A,2,04,,,,,,,,,,,,2.5,1.3,2.1*31");
        assert_eq!(two.fix_mode, Some(FixMode::TwoD));
        let none = one(&mut d, "$GNGSA,A,1,,,,,,,,,,,,,99.99,99.99,99.99,1*33");
        assert_eq!(none.fix_mode, Some(FixMode::None));

        let mut d = Decoder::default();
        d.push(b"$GPGSV,1,1,02,04,70,050,45,05,44,120,40*7E\r\n");
        d.push(b"$GPGSA,A,3,04,,,,,,,,,,,,2.5,1.3,2.1*30\r\n");
        let used: Vec<(f64, bool)> = d
            .satellites_in_view()
            .iter()
            .map(|s| (s.id, s.used))
            .collect();
        assert_eq!(used, [(4.0, true), (5.0, false)]);

        let bare = one(&mut Decoder::default(), "$GPGSA,A,3,04,,,,,,,,,,,,,,*18");
        assert_eq!(bare.fix_mode, Some(FixMode::ThreeD));
        assert_eq!((bare.pdop, bare.hdop, bare.vdop), (None, None, None));
    }

    #[test]
    fn a_constellation_is_published_once_its_sweep_completes() {
        let mut d = Decoder::default();
        let first = "$GPGSV,2,1,05,04,70,050,45,05,44,120,40,09,30,200,38,12,20,290,33*73\r\n";
        assert!(d.push(first.as_bytes()).is_empty());
        assert!(d.satellites_in_view().is_empty());
        let second = d.push(b"$GPGSV,2,2,05,24,10,330,28*41\r\n");
        assert_eq!(second[0].satellites.as_ref().map(Vec::len), Some(5));
        assert_eq!(d.satellites_in_view().len(), 5);

        let mut d = Decoder::default();
        for line in [
            "$GPGSV,1,1,01,04,70,050,45*4F",
            "$GLGSV,1,1,01,68,40,100,42*59",
            "$GAGSV,1,1,01,07,60,100,44*59",
            "$GBGSV,1,1,01,21,50,150,40*5C",
        ] {
            d.push(format!("{line}\r\n").as_bytes());
        }
        let systems: Vec<Constellation> = d
            .satellites_in_view()
            .iter()
            .map(|s| s.constellation)
            .collect();
        use Constellation as C;
        assert_eq!(systems, [C::Beidou, C::Galileo, C::Glonass, C::Gps]);
        assert_eq!(
            d.constellations_seen(),
            [C::Beidou, C::Galileo, C::Glonass, C::Gps]
        );

        let mut d = Decoder::default();
        d.push(b"$GPGSV,1,1,01,04,70,050,45*4F\r\n");
        let sat = &d.satellites_in_view()[0];
        assert_eq!(
            (sat.id, sat.elevation, sat.azimuth, sat.snr),
            (4.0, Some(70.0), Some(50.0), Some(45.0))
        );
        d.push(b"$GPGSV,1,1,00,1*64\r\n");
        assert!(d.satellites_in_view().is_empty());
        assert_eq!(d.constellations_seen(), [C::Gps]);

        let mut d = Decoder::default();
        d.push(b"$XXGSV,1,1,01,04,70,050,45*58\r\n");
        assert_eq!(d.satellites_in_view()[0].constellation, C::Unknown);

        assert!(
            Decoder::default()
                .push(b"$GPGSV,,,01,04,70,050,45*4F\r\n")
                .is_empty()
        );

        let mut d = Decoder::default();
        d.push(b"$GPGSV,2,1,05,04,70,050,45*48\r\n");
        d.push(b"$GPGSV,2,1,02,09,30,200,38*4B\r\n");
        d.push(b"$GPGSV,2,2,02,12,20,290,33*41\r\n");
        let ids: Vec<f64> = d.satellites_in_view().iter().map(|s| s.id).collect();
        assert_eq!(ids, [9.0, 12.0]);

        let u = one(&mut Decoder::default(), "$GPGSV,2,2,02,24,10,330,28*46");
        assert_eq!(u.satellites.map(|s| s.len()), Some(1));
    }

    #[test]
    fn gaps_and_repeats_in_a_sweep() {
        let mut d = Decoder::default();
        d.push(b"$GPGSV,1,1,02,04,,,,05,44,120,40*4D\r\n");
        let snrs: Vec<Option<f64>> = d.satellites_in_view().iter().map(|s| s.snr).collect();
        assert_eq!(snrs, [None, Some(40.0)]);

        let mut d = Decoder::default();
        d.push(b"$GPGSV,1,1,02,,,,,05,44,120,40*49\r\n");
        assert_eq!(
            d.satellites_in_view()
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            [5.0]
        );

        let strongest = |line: &str| {
            let mut d = Decoder::default();
            d.push(format!("{line}\r\n").as_bytes());
            let sats = d.satellites_in_view();
            assert_eq!(sats.len(), 1, "{line}");
            (sats[0].snr, sats[0].elevation)
        };
        assert_eq!(
            strongest("$GPGSV,1,1,02,04,70,050,30,04,70,050,48*74"),
            (Some(48.0), Some(70.0))
        );
        assert_eq!(
            strongest("$GPGSV,1,1,02,04,70,050,48,04,70,050,30*74"),
            (Some(48.0), Some(70.0))
        );
        assert_eq!(
            strongest("$GPGSV,1,1,02,04,70,050,45,04,70,050,*7A"),
            (Some(45.0), Some(70.0))
        );
        assert_eq!(
            strongest("$GPGSV,1,1,02,04,70,050,40,04,10,200,40*7A"),
            (Some(40.0), Some(70.0))
        );
        assert_eq!(
            strongest("$GPGSV,1,1,02,04,70,050,,04,10,200,40*7E"),
            (Some(40.0), Some(10.0))
        );
    }

    #[test]
    fn gst_gives_the_horizontal_accuracy() {
        let u = one(
            &mut Decoder::default(),
            "$GPGST,123519.00,1.5,,,,3.0,4.0,2.0*750",
        );
        assert!(close(u.gps.and_then(|g| g.accuracy_m), 5.0, 5));
        assert!(
            Decoder::default()
                .push(b"$GPGST,123519.00,1.5,,,,,,*5E\r\n")
                .is_empty()
        );
    }

    #[test]
    fn malformed_fields_count_as_absent() {
        let u = one(
            &mut Decoder::default(),
            "$GPGGA,123519.00,4807.038,N,01131.000,E,,,,,M,,M,,*4C",
        );
        assert_eq!(
            (u.fix_quality, u.satellites_used, u.hdop),
            (Some(FixQuality::None), None, None)
        );

        let u = one(
            &mut Decoder::default(),
            "$GPGGA,123519.00,4807.038,N,01131.000,E,1,,,,M,,M,,*7D",
        );
        let gps = u.gps.unwrap();
        assert!(gps.lat.is_some());
        assert_eq!((gps.alt, gps.satellites), (None, None));

        let u = one(
            &mut Decoder::default(),
            "$GPRMC,123519.00,A,,,,,022.4,084.4,230326,,,A*62",
        );
        assert_eq!((u.gps, u.receiver_time.is_some()), (None, true));

        let u = one(
            &mut Decoder::default(),
            "$GPRMC,123519.00,A,4807.038,N,01131.000,E,,,230326,,,A*57",
        );
        let gps = u.gps.unwrap();
        assert!(close(gps.lat, 48.1173, 3));
        assert_eq!((gps.speed_ms, gps.heading), (None, None));

        let u = one(
            &mut Decoder::default(),
            "$GPGGA,123519.00,abc,N,def,E,1,xx,yy,zz,M,,M,,*48",
        );
        assert_eq!((u.gps, u.hdop), (None, None));

        let u = one(
            &mut Decoder::default(),
            "$GPGGA,123519.00,9e999,N,01131.000,E,1,08,0.9,545.4,M,,M,,*07",
        );
        assert_eq!(u.gps, None);

        let u = one(
            &mut Decoder::default(),
            "$GPRMC,123519.00,A,4807.038,N,01131.000,E,022.4,084.4,abcdef,,,A*5A",
        );
        assert_eq!(u.receiver_time, None);
    }

    #[test]
    fn numbers_are_read_like_the_fields_always_were() {
        let cases: [(&str, Option<f64>); 16] = [
            ("12", Some(12.0)),
            (" 12\t", Some(12.0)),
            ("+1.5", Some(1.5)),
            ("-.5", Some(-0.5)),
            ("5.", Some(5.0)),
            ("1e3", Some(1000.0)),
            ("0x1A", Some(26.0)),
            ("0b101", Some(5.0)),
            ("0o17", Some(15.0)),
            ("0x", None),
            ("-0x10", None),
            ("abc", None),
            (".", None),
            ("1e", None),
            ("inf", None),
            ("  ", Some(0.0)),
        ];
        for (text, want) in cases {
            assert_eq!(number(text), want, "{text}");
        }
        assert_eq!(number("Infinity"), Some(f64::INFINITY));
        assert_eq!(num(Some("Infinity")), None);
        assert_eq!(num(Some("")), None);
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(-2.5), -2.0);
        assert_eq!(round_half_up(0.499_999_999_999_999_94), 0.0);
    }

    #[test]
    fn dates_carry_over_like_a_calendar() {
        assert_eq!(date_utc(2026.0, 2.0, 23.0, 12.0, 35.0, 19.0, 0.0), RMC_TIME);
        assert_eq!(date_utc(1970.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0), 0.0);
        assert_eq!(
            date_utc(2000.0, 1.0, 29.0, 0.0, 0.0, 0.0, 0.0),
            951_782_400_000.0
        );
        assert_eq!(
            date_utc(2026.0, 12.0, 0.0, 25.0, 0.0, 0.0, 0.0),
            1_798_765_200_000.0
        );
        assert_eq!(
            date_utc(1999.0, -1.0, 31.0, -1.0, 61.0, 59.0, 999.0),
            915_062_519_999.0
        );
        assert_eq!(
            date_utc(2099.0, 98.0, 99.0, 99.0, 99.0, 99.0, 1000.0),
            4_337_210_440_000.0
        );
        assert!(date_utc(2026.0, 0.0, 1.0, 0.0, 0.0, 1e300, 0.0).is_nan());
        assert_eq!(utc_ms(Some("123519.123"), "230326"), Some(RMC_TIME + 123.0));
        assert_eq!(utc_ms(Some("12351"), "230326"), None);
        assert_eq!(utc_ms(Some("123519"), ""), None);
    }
}
