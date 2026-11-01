// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Sunrise / sunset day-night switching (`appearanceMode: "sun"`).
//!
//! Once a minute the sun's elevation at the car's position decides whether it
//! is night. A transition is pushed into telemetry as `nightMode` (phones,
//! cluster, red night accent) and `darkMode` (LIVI theme), so everything
//! switches together. Position comes from the GNSS fix in telemetry, else the
//! configured `sunLatitude` / `sunLongitude`; with neither (0/0 = unset)
//! nothing is switched. Nothing is switched either until the wall clock is
//! trusted, see [`ClockTrust`].

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_core_proto::config::{AppearanceMode, Config};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::gnss::clock::CLAIM_FILE as GNSS_CLAIM_FILE;
use crate::server::Core;

const TICK: Duration = Duration::from_secs(60);
const SUNSET_ALTITUDE_DEG: f64 = -0.833;
const DAY_MS: f64 = 86_400_000.0;

pub const MIN_PLAUSIBLE_CLOCK_MS: i64 = 1_767_225_600_000;
const RTC_MAX_SKEW_MS: i64 = 5 * 60_000;
const RTC_EPOCH_PATH: &str = "/sys/class/rtc/rtc0/since_epoch";
const TIMESYNCD_MARKER: &str = "/run/systemd/timesync/synchronized";
const CHRONY_RUN_DIR: &str = "/run/chrony";
const PHONE_CLAIM_FILE: &str = "/tmp/livi-phone-clock";
const UPTIME_PATH: &str = "/proc/uptime";
const BOOT_SLACK_S: f64 = 5.0;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn solar_elevation_deg(unix_ms: i64, lat: f64, lng: f64) -> f64 {
    let d = unix_ms as f64 / DAY_MS + 2_440_587.5 - 2_451_545.0;
    let g = (357.529 + 0.985_600_28 * d).to_radians();
    let q = 280.459 + 0.985_647_36 * d;
    let l = (q + 1.915 * g.sin() + 0.02 * (2.0 * g).sin()).to_radians();
    let e = (23.439 - 0.000_000_36 * d).to_radians();

    let ra = (e.cos() * l.sin()).atan2(l.cos());
    let dec = (e.sin() * l.sin()).asin();

    let gmst_deg = (18.697_374_558 + 24.065_709_824_419_08 * d) * 15.0;
    let ha = (gmst_deg + lng).to_radians() - ra;

    let lat = lat.to_radians();
    (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos())
        .asin()
        .to_degrees()
}

pub fn is_night_at(unix_ms: i64, lat: f64, lng: f64) -> bool {
    solar_elevation_deg(unix_ms, lat, lng) < SUNSET_ALTITUDE_DEG
}

fn valid(lat: f64, lng: f64) -> bool {
    lat.is_finite()
        && lng.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lng)
        && !(lat == 0.0 && lng == 0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationSource {
    Gps,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunLocation {
    pub lat: f64,
    pub lng: f64,
    pub source: LocationSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    pub active: bool,
    pub lat: f64,
    pub lng: f64,
}

impl Setup {
    pub fn of(cfg: &Config) -> Self {
        Self {
            active: cfg.appearance_mode == AppearanceMode::Sun,
            lat: cfg.sun_latitude,
            lng: cfg.sun_longitude,
        }
    }
}

pub fn resolve_location(gps: Option<&Value>, setup: &Setup) -> Option<SunLocation> {
    let fix = gps.and_then(|g| Some((g.get("lat")?.as_f64()?, g.get("lng")?.as_f64()?)));
    if let Some((lat, lng)) = fix.filter(|(lat, lng)| valid(*lat, *lng)) {
        return Some(SunLocation {
            lat,
            lng,
            source: LocationSource::Gps,
        });
    }
    valid(setup.lat, setup.lng).then_some(SunLocation {
        lat: setup.lat,
        lng: setup.lng,
        source: LocationSource::Manual,
    })
}

#[derive(Debug, Default)]
pub struct Schedule {
    last: Option<bool>,
}

impl Schedule {
    pub fn evaluate(
        &mut self,
        setup: &Setup,
        gps: Option<&Value>,
        unix_ms: i64,
        trusted: impl FnOnce() -> bool,
    ) -> Option<bool> {
        if !setup.active {
            self.last = None;
            return None;
        }
        let loc = resolve_location(gps, setup)?;
        if !trusted() {
            return None;
        }
        let night = is_night_at(unix_ms, loc.lat, loc.lng);
        if self.last == Some(night) {
            return None;
        }
        self.last = Some(night);
        Some(night)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    Host,
    Rtc,
    Ntp,
    Gnss,
    Phone,
}

type ReadFile = Box<dyn Fn(&str) -> Option<String> + Send>;
type FileExists = Box<dyn Fn(&str) -> bool + Send>;
type Chrony = Box<dyn Fn() -> Option<String> + Send>;

pub struct ClockTrust {
    read_file: ReadFile,
    file_exists: FileExists,
    this_boot: FileExists,
    chrony: Chrony,
    trusted: Option<ClockSource>,
    warned: bool,
}

pub fn is_clock_plausible(unix_ms: i64) -> bool {
    unix_ms >= MIN_PLAUSIBLE_CLOCK_MS
}

fn written_this_boot(path: &str) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    let uptime = std::fs::read_to_string(UPTIME_PATH)
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok());
    let boot = uptime
        .and_then(|up| Duration::try_from_secs_f64(up + BOOT_SLACK_S).ok())
        .and_then(|up| SystemTime::now().checked_sub(up));
    boot.is_some_and(|boot| modified >= boot)
}

fn chrony_tracking() -> Option<String> {
    if !Path::new(CHRONY_RUN_DIR).exists() {
        return None;
    }
    let out = std::process::Command::new("chronyc")
        .args(["-n", "-c", "tracking"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn chrony_synced(out: &str) -> bool {
    let fields: Vec<&str> = out.trim().split(',').collect();
    let stratum = fields.get(2).and_then(|s| s.trim().parse::<u32>().ok());
    let leap = fields.last().map(|s| s.trim());
    stratum.is_some_and(|s| s > 0 && s < 16) && leap == Some("Normal")
}

impl ClockTrust {
    pub fn system() -> Self {
        Self::new(
            cfg!(target_os = "linux"),
            Box::new(|p| std::fs::read_to_string(p).ok()),
            Box::new(|p| Path::new(p).exists()),
            Box::new(written_this_boot),
            Box::new(chrony_tracking),
        )
    }

    pub fn new(
        linux: bool,
        read_file: ReadFile,
        file_exists: FileExists,
        this_boot: FileExists,
        chrony: Chrony,
    ) -> Self {
        Self {
            read_file,
            file_exists,
            this_boot,
            chrony,
            trusted: (!linux).then_some(ClockSource::Host),
            warned: false,
        }
    }

    fn rtc_ok(&self, sys_ms: i64) -> bool {
        let Some(raw) = (self.read_file)(RTC_EPOCH_PATH) else {
            return false;
        };
        let Ok(secs) = raw.trim().parse::<i64>() else {
            return false;
        };
        let rtc_ms = secs.saturating_mul(1000);
        is_clock_plausible(rtc_ms) && (rtc_ms - sys_ms).abs() <= RTC_MAX_SKEW_MS
    }

    fn source(&self, sys_ms: i64) -> Option<ClockSource> {
        if self.rtc_ok(sys_ms) {
            return Some(ClockSource::Rtc);
        }
        if (self.file_exists)(TIMESYNCD_MARKER)
            || (self.chrony)().is_some_and(|o| chrony_synced(&o))
        {
            return Some(ClockSource::Ntp);
        }
        if (self.this_boot)(GNSS_CLAIM_FILE) {
            return Some(ClockSource::Gnss);
        }
        (self.this_boot)(PHONE_CLAIM_FILE).then_some(ClockSource::Phone)
    }

    pub fn check(&mut self, sys_ms: i64) -> Option<ClockSource> {
        if self.trusted.is_some() {
            return self.trusted;
        }
        if !is_clock_plausible(sys_ms) {
            return None;
        }
        self.trusted = self.source(sys_ms);
        match self.trusted {
            Some(source) => println!("[sun] system clock trusted (source: {source:?})"),
            None if !self.warned => {
                self.warned = true;
                println!(
                    "[sun] clock not confirmed by RTC, NTP, GPS or phone, sunrise/sunset switching on hold"
                );
            }
            None => {}
        }
        self.trusted
    }
}

pub async fn run(core: Arc<Core>, telemetry: mpsc::UnboundedSender<Value>) {
    let mut state = core.hub.watch();
    let mut setup = Setup::of(&state.borrow_and_update().config);
    let mut trust = ClockTrust::system();
    let mut schedule = Schedule::default();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let next = Setup::of(&state.borrow_and_update().config);
                if next == setup {
                    continue;
                }
                setup = next;
            }
        }
        let gps = state.borrow().telemetry.get("gps").cloned();
        let now = now_ms();
        if let Some(night) =
            schedule.evaluate(&setup, gps.as_ref(), now, || trust.check(now).is_some())
        {
            println!("[sun] switching to {}", if night { "night" } else { "day" });
            let _ = telemetry.send(json!({
                "nightMode": night,
                "darkMode": night,
                "clusterNightMode": null,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use serde_json::json;

    use super::*;

    const AMS: (f64, f64) = (52.37, 4.9);

    fn at(iso: &str) -> i64 {
        iso.parse::<jiff::Timestamp>().unwrap().as_millisecond()
    }

    fn night(iso: &str) -> bool {
        is_night_at(at(iso), AMS.0, AMS.1)
    }

    #[test]
    fn midsummer_noon_in_amsterdam_is_high_in_the_sky() {
        let elev = solar_elevation_deg(at("2026-06-21T11:40:00Z"), AMS.0, AMS.1);
        assert!(elev > 58.0 && elev < 63.0, "{elev}");
    }

    #[test]
    fn day_and_night_around_amsterdam_sunrise_and_sunset() {
        assert!(!night("2026-06-21T12:00:00Z"));
        assert!(night("2026-06-21T23:00:00Z"));
        assert!(night("2026-12-21T07:15:00Z"));
        assert!(!night("2026-12-21T09:00:00Z"));
        assert!(!night("2026-12-21T15:00:00Z"));
        assert!(night("2026-12-21T16:00:00Z"));
    }

    fn setup(lat: f64, lng: f64) -> Setup {
        Setup {
            active: true,
            lat,
            lng,
        }
    }

    #[test]
    fn gps_wins_over_the_manual_location() {
        let loc = resolve_location(Some(&json!({ "lat": 1.0, "lng": 2.0 })), &setup(3.0, 4.0));
        assert_eq!(
            loc,
            Some(SunLocation {
                lat: 1.0,
                lng: 2.0,
                source: LocationSource::Gps
            })
        );
    }

    #[test]
    fn falls_back_to_manual_and_treats_zero_zero_as_unset() {
        let manual = resolve_location(None, &setup(3.0, 4.0));
        assert_eq!(manual.map(|l| l.source), Some(LocationSource::Manual));
        let zero = json!({ "lat": 0.0, "lng": 0.0 });
        assert_eq!(resolve_location(Some(&zero), &setup(0.0, 0.0)), None);
        let off_globe = json!({ "lat": 95.0, "lng": 2.0 });
        assert_eq!(resolve_location(Some(&off_globe), &setup(0.0, 0.0)), None);
        let partial = json!({ "lat": 52.0 });
        assert_eq!(
            resolve_location(Some(&partial), &setup(3.0, 4.0)).map(|l| l.source),
            Some(LocationSource::Manual)
        );
    }

    #[test]
    fn without_any_location_nothing_is_switched() {
        let mut s = Schedule::default();
        assert_eq!(
            s.evaluate(&setup(0.0, 0.0), None, at("2026-06-21T23:00:00Z"), || true),
            None
        );
    }

    #[test]
    fn switches_at_once_and_then_only_on_transitions() {
        let mut s = Schedule::default();
        let ams = setup(AMS.0, AMS.1);
        assert_eq!(
            s.evaluate(&ams, None, at("2026-06-21T12:00:00Z"), || true),
            Some(false)
        );
        assert_eq!(
            s.evaluate(&ams, None, at("2026-06-21T12:01:00Z"), || true),
            None
        );
        assert_eq!(
            s.evaluate(&ams, None, at("2026-06-21T23:00:00Z"), || true),
            Some(true)
        );
    }

    #[test]
    fn idle_outside_sun_mode_and_fresh_when_switched_back() {
        let mut s = Schedule::default();
        let day_mode = Setup {
            active: false,
            ..setup(AMS.0, AMS.1)
        };
        assert_eq!(
            s.evaluate(&day_mode, None, at("2026-06-21T23:00:00Z"), || true),
            None
        );
        let ams = setup(AMS.0, AMS.1);
        assert_eq!(
            s.evaluate(&ams, None, at("2026-06-21T23:00:00Z"), || true),
            Some(true)
        );
        assert_eq!(
            s.evaluate(&day_mode, None, at("2026-06-22T01:00:00Z"), || true),
            None
        );
        assert_eq!(
            s.evaluate(&ams, None, at("2026-06-22T01:00:00Z"), || true),
            Some(true)
        );
    }

    #[test]
    fn holds_the_last_state_until_the_clock_is_trusted() {
        let mut s = Schedule::default();
        let ams = setup(AMS.0, AMS.1);
        let noon = at("2026-06-21T12:00:00Z");
        assert_eq!(s.evaluate(&ams, None, noon, || false), None);
        assert_eq!(s.evaluate(&ams, None, noon, || true), Some(false));
    }

    #[test]
    fn a_gps_fix_moves_the_sun() {
        let mut s = Schedule::default();
        let sydney = json!({ "lat": -33.87, "lng": 151.21 });
        assert_eq!(
            s.evaluate(
                &setup(AMS.0, AMS.1),
                Some(&sydney),
                at("2026-06-21T12:00:00Z"),
                || true
            ),
            Some(true)
        );
    }

    const NOW: i64 = 1_782_043_200_000;

    fn secs(ms: i64) -> String {
        (ms / 1000).to_string()
    }

    fn linux(read: Option<String>, exists: fn(&str) -> bool, chrony: Option<&str>) -> ClockTrust {
        let chrony = chrony.map(str::to_string);
        ClockTrust::new(
            true,
            Box::new(move |_| read.clone()),
            Box::new(exists),
            Box::new(exists),
            Box::new(move || chrony.clone()),
        )
    }

    #[test]
    fn the_test_clock_is_midsummer_noon() {
        assert_eq!(NOW, at("2026-06-21T12:00:00Z"));
        assert_eq!(MIN_PLAUSIBLE_CLOCK_MS, at("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn pre_2026_clocks_are_implausible() {
        assert!(!is_clock_plausible(0));
        assert!(is_clock_plausible(NOW));
    }

    #[test]
    fn hosts_other_than_linux_are_always_trusted() {
        let mut t = ClockTrust::new(
            false,
            Box::new(|_| None),
            Box::new(|_| false),
            Box::new(|_| false),
            Box::new(|| None),
        );
        assert_eq!(t.check(0), Some(ClockSource::Host));
    }

    #[test]
    fn untrusted_on_linux_without_rtc_ntp_or_gps() {
        assert_eq!(linux(None, |_| false, None).check(NOW), None);
    }

    #[test]
    fn an_implausible_system_clock_is_never_trusted() {
        assert_eq!(linux(None, |_| true, None).check(0), None);
    }

    #[test]
    fn a_plausible_rtc_close_to_the_system_clock_is_trusted() {
        let rtc = Some(format!("{}\n", secs(NOW + 30_000)));
        assert_eq!(
            linux(rtc, |_| false, None).check(NOW),
            Some(ClockSource::Rtc)
        );
    }

    #[test]
    fn an_rtc_reset_to_1970_is_not_trusted() {
        assert_eq!(linux(Some("12\n".into()), |_| false, None).check(NOW), None);
    }

    #[test]
    fn an_rtc_far_from_the_system_clock_is_not_trusted() {
        let rtc = Some(secs(NOW - 3_600_000));
        assert_eq!(linux(rtc, |_| false, None).check(NOW), None);
    }

    #[test]
    fn the_timesyncd_marker_means_ntp_synced() {
        let mut t = linux(None, |p| p == TIMESYNCD_MARKER, None);
        assert_eq!(t.check(NOW), Some(ClockSource::Ntp));
    }

    #[test]
    fn chrony_with_a_normal_leap_status_means_ntp_synced() {
        let ok = "C0A80001,192.168.0.1,3,1782043200.000000000,0.000001,0.0,0.0,1.0,0.0,0.0,0.01,0.001,64.0,Normal\n";
        assert_eq!(
            linux(None, |_| false, Some(ok)).check(NOW),
            Some(ClockSource::Ntp)
        );
        let unsync = "00000000,,0,0.000000000,0,0,0,0,0,0,0,0,0,Not synchronised\n";
        assert_eq!(linux(None, |_| false, Some(unsync)).check(NOW), None);
    }

    #[test]
    fn a_gps_disciplined_clock_is_trusted() {
        let mut t = linux(None, |p| p == GNSS_CLAIM_FILE, None);
        assert_eq!(t.check(NOW), Some(ClockSource::Gnss));
    }

    #[test]
    fn a_clock_set_by_the_phone_is_trusted() {
        let mut t = linux(None, |p| p == PHONE_CLAIM_FILE, None);
        assert_eq!(t.check(NOW), Some(ClockSource::Phone));
    }

    #[test]
    fn stays_trusted_once_confirmed() {
        static SYNCED: AtomicBool = AtomicBool::new(true);
        let mut t = linux(None, |_| SYNCED.load(Ordering::SeqCst), None);
        assert_eq!(t.check(NOW), Some(ClockSource::Ntp));
        SYNCED.store(false, Ordering::SeqCst);
        assert_eq!(t.check(NOW), Some(ClockSource::Ntp));
    }
}
