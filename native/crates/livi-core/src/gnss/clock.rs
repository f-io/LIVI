// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::path::{Path, PathBuf};

use super::SET_TIME_HELPER;
use super::info::{FixMode, GnssInfo};
use super::nmea::round_half_up;
use crate::privileged::sudo;

pub const CLAIM_FILE: &str = "/tmp/livi-gps-clock";

const DRIFT_THRESHOLD_S: f64 = 2.0;
const GROSS_DRIFT_S: f64 = 60.0;
const MIN_STEP_INTERVAL_MS: i64 = 60_000;
const CLAIM_REFRESH_MS: i64 = 30_000;

pub fn trustworthy(info: &GnssInfo) -> bool {
    info.receiver_time.is_some()
        && info.fix_mode == FixMode::ThreeD
        && info.satellites_used >= 4.0
        && info.hdop.is_none_or(|h| h <= 5.0)
}

fn may_step(info: &GnssInfo, drift_s: f64) -> bool {
    if drift_s.abs() > GROSS_DRIFT_S {
        return info.receiver_time.is_some();
    }
    drift_s.abs() > DRIFT_THRESHOLD_S && trustworthy(info)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub epoch: i64,
    pub drift_s: f64,
}

pub struct Clock {
    claim_file: PathBuf,
    claimed: bool,
    last_claim_at: i64,
    last_step_at: i64,
}

impl Clock {
    pub fn new(claim_file: impl Into<PathBuf>) -> Self {
        Self {
            claim_file: claim_file.into(),
            claimed: false,
            last_claim_at: 0,
            last_step_at: 0,
        }
    }

    pub fn update(&mut self, info: &GnssInfo, now_ms: i64) -> Option<Step> {
        let Some(receiver_time) = info.receiver_time else {
            self.release();
            return None;
        };
        if trustworthy(info) {
            self.claim(now_ms);
        } else {
            self.release();
        }
        let drift_s = (receiver_time - now_ms as f64) / 1000.0;
        if !may_step(info, drift_s) || now_ms - self.last_step_at < MIN_STEP_INTERVAL_MS {
            return None;
        }
        self.last_step_at = now_ms;
        Some(Step {
            epoch: round_half_up(receiver_time / 1000.0) as i64,
            drift_s,
        })
    }

    pub fn release(&mut self) {
        if !self.claimed {
            return;
        }
        self.claimed = false;
        let _ = fs::remove_file(&self.claim_file);
    }

    fn claim(&mut self, now_ms: i64) {
        if self.claimed && now_ms - self.last_claim_at < CLAIM_REFRESH_MS {
            return;
        }
        self.last_claim_at = now_ms;
        match fs::write(&self.claim_file, format!("{now_ms}\n")) {
            Ok(()) => self.claimed = true,
            Err(e) => eprintln!("[clock] could not claim the clock: {e}"),
        }
    }
}

pub async fn step(step: Step) {
    if !Path::new(SET_TIME_HELPER).exists() {
        eprintln!(
            "[clock] {SET_TIME_HELPER} missing, run the installer again to let GPS set the time"
        );
        return;
    }
    if sudo(&[SET_TIME_HELPER, &step.epoch.to_string()]).await {
        println!(
            "[clock] system clock stepped by {:.1}s from GPS",
            step.drift_s
        );
    } else {
        eprintln!("[clock] setting the system clock from GPS failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;
    use crate::gnss::info::FixQuality;

    const NOW: i64 = 1_800_000_000_000;

    fn good_fix() -> GnssInfo {
        GnssInfo {
            connected: true,
            fix_mode: FixMode::ThreeD,
            fix_quality: FixQuality::Gps,
            satellites_used: 8.0,
            hdop: Some(0.9),
            receiver_time: Some(NOW as f64),
            ..Default::default()
        }
    }

    fn at(receiver_time: i64) -> GnssInfo {
        GnssInfo {
            receiver_time: Some(receiver_time as f64),
            ..good_fix()
        }
    }

    fn no_lock(receiver_time: i64) -> GnssInfo {
        GnssInfo {
            fix_mode: FixMode::None,
            satellites_used: 0.0,
            ..at(receiver_time)
        }
    }

    #[test]
    fn only_a_3d_lock_on_enough_satellites_is_trusted() {
        assert!(trustworthy(&good_fix()));
        assert!(trustworthy(&GnssInfo {
            hdop: None,
            ..good_fix()
        }));
        assert!(!trustworthy(&GnssInfo {
            receiver_time: None,
            ..good_fix()
        }));
        assert!(!trustworthy(&GnssInfo {
            fix_mode: FixMode::TwoD,
            ..good_fix()
        }));
        assert!(!trustworthy(&GnssInfo {
            satellites_used: 3.0,
            ..good_fix()
        }));
        assert!(!trustworthy(&GnssInfo {
            hdop: Some(9.0),
            ..good_fix()
        }));
    }

    #[test]
    fn the_claim_follows_the_lock() {
        let dir = TempDir::new();
        let file = dir.0.join("claim");
        let mut clock = Clock::new(&file);
        clock.update(&good_fix(), NOW);
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{NOW}\n"));
        clock.update(&good_fix(), NOW + 1000);
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{NOW}\n"));
        clock.update(&good_fix(), NOW + 31_000);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("{}\n", NOW + 31_000)
        );

        clock.update(
            &GnssInfo {
                fix_mode: FixMode::None,
                ..good_fix()
            },
            NOW + 32_000,
        );
        assert!(!file.exists());
        clock.update(&good_fix(), NOW + 33_000);
        assert!(file.exists());
        clock.update(
            &GnssInfo {
                receiver_time: None,
                ..good_fix()
            },
            NOW + 34_000,
        );
        assert!(!file.exists());

        let mut never = Clock::new(dir.0.join("never"));
        never.release();
        never.update(&no_lock(NOW + 3_600_000), NOW);
        assert!(!dir.0.join("never").exists());

        let mut stuck = Clock::new(dir.0.join("missing/claim"));
        stuck.update(&good_fix(), NOW);
        stuck.release();
    }

    #[test]
    fn the_clock_is_stepped_on_real_drift_and_not_too_often() {
        let dir = TempDir::new();
        let mut clock = Clock::new(dir.0.join("claim"));
        assert_eq!(clock.update(&at(NOW + 1500), NOW), None);
        assert_eq!(
            clock.update(&at(NOW + 60_000), NOW),
            Some(Step {
                epoch: (NOW + 60_000) / 1000,
                drift_s: 60.0
            })
        );
        assert_eq!(clock.update(&at(NOW + 65_000), NOW + 5000), None);
        assert!(clock.update(&at(NOW + 121_000), NOW + 61_000).is_some());
    }

    #[test]
    fn a_plainly_wrong_clock_is_set_without_a_lock() {
        let dir = TempDir::new();
        let mut clock = Clock::new(dir.0.join("claim"));
        assert_eq!(
            clock.update(&no_lock(NOW + 3_600_000), NOW),
            Some(Step {
                epoch: (NOW + 3_600_000) / 1000,
                drift_s: 3600.0
            })
        );
        let mut clock = Clock::new(dir.0.join("claim"));
        assert_eq!(clock.update(&no_lock(NOW + 10_000), NOW), None);
        assert_eq!(clock.update(&at(NOW + 500), NOW + 499), None);
        assert_eq!(
            clock.update(&at(NOW + 1_500_500), NOW).map(|s| s.epoch),
            Some(1_800_001_501)
        );
    }
}
