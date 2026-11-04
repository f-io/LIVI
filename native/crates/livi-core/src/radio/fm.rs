// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{Duration, Instant};

use livi_core_proto::state::{FmSignal, FmStation};
use livi_fm::{HIGHEST_KHZ, LOWEST_KHZ, STEP_KHZ, Signal};

/// About 100 ms of the band at one frequency pick the frequencies worth a closer listen.
pub const MEASURE_BLOCKS: usize = 6;
/// Empty channels measure about −6 dB.
const CANDIDATE_NOISE_DB: f32 = -10.0;
/// A station beside the tuned frequency also quiets the demodulator, but pulls it off centre.
const CANDIDATE_OFFSET_HZ: f32 = 30_000.0;
/// A station shows its stereo pilot or RDS within this.
const CONFIRM_WAIT: Duration = Duration::from_millis(1000);
const RDS_SYNC: Duration = Duration::from_millis(1200);
/// Long enough to see a station scroll titles through its name.
const NAME_WAIT: Duration = Duration::from_millis(2500);
const RDS_GOOD: u8 = 50;
const QUIET_DB: f32 = 40.0;

pub fn count() -> usize {
    livi_fm::frequencies_khz().count()
}

pub fn khz_at(index: usize) -> u32 {
    LOWEST_KHZ + index as u32 * STEP_KHZ
}

pub fn in_band(khz: u32) -> bool {
    (LOWEST_KHZ..=HIGHEST_KHZ).contains(&khz) && (khz - LOWEST_KHZ).is_multiple_of(STEP_KHZ)
}

pub fn mhz(khz: u32) -> String {
    format!("{}.{}", khz / 1000, khz % 1000 / 100)
}

/// A seek runs round the band.
pub fn seek_next(khz: u32, up: bool) -> u32 {
    match up {
        true if khz >= HIGHEST_KHZ => LOWEST_KHZ,
        true => khz + STEP_KHZ,
        false if khz <= LOWEST_KHZ => HIGHEST_KHZ,
        false => khz - STEP_KHZ,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Wait,
    Station,
    Nothing,
}

/// One frequency of a scan or a seek: measured first, then listened to for a stereo pilot
/// or RDS, which only a station sends.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe {
    fed: usize,
    heard: Option<Instant>,
    stereo: bool,
    rds: bool,
}

impl Probe {
    pub fn measuring(&self) -> bool {
        self.heard.is_none()
    }

    /// A seek needs no name, the pilot or RDS is enough.
    pub fn shows_station(&self) -> bool {
        self.stereo || self.rds
    }

    /// A gain change spoils a measurement, not a listen.
    pub fn restart(&mut self) -> bool {
        if self.measuring() {
            self.fed = 0;
        }
        self.measuring()
    }

    pub fn step(&mut self, s: &Signal, synced: bool, now: Instant) -> Verdict {
        let Some(at) = self.heard else {
            self.fed += 1;
            if self.fed < MEASURE_BLOCKS {
                return Verdict::Wait;
            }
            if !candidate(s) {
                return Verdict::Nothing;
            }
            self.heard = Some(now);
            return Verdict::Wait;
        };
        self.stereo |= s.stereo;
        self.rds |= synced;
        verdict(now.duration_since(at), self.stereo, self.rds)
    }
}

fn candidate(s: &Signal) -> bool {
    s.noise_db < CANDIDATE_NOISE_DB && s.offset_hz.abs() < CANDIDATE_OFFSET_HZ
}

/// With RDS the listen lasts until the name is clear, without it ends once the pilot showed.
fn verdict(listened: Duration, stereo: bool, rds: bool) -> Verdict {
    let station = stereo || rds;
    if listened >= NAME_WAIT || (stereo && !rds && listened >= RDS_SYNC) {
        return if station {
            Verdict::Station
        } else {
            Verdict::Nothing
        };
    }
    if !station && listened >= CONFIRM_WAIT {
        return Verdict::Nothing;
    }
    Verdict::Wait
}

pub fn signal(s: &Signal) -> FmSignal {
    let quality = (-s.noise_db / QUIET_DB * 100.0).clamp(0.0, 100.0) as u32;
    FmSignal {
        quality,
        stereo: s.stereo,
        rds: s.rds >= RDS_GOOD,
    }
}

/// One programme heard on several frequencies stays once, on the cleanest.
pub fn keep_found(found: &mut Vec<(FmStation, f32)>, station: FmStation, noise_db: f32) {
    let same = station
        .pi
        .and_then(|pi| found.iter().position(|(f, _)| f.pi == Some(pi)));
    match same {
        Some(at) if found[at].1 <= noise_db => {}
        Some(at) => found[at] = (station, noise_db),
        None => found.push((station, noise_db)),
    }
}

/// Elsewhere other stations share the frequencies, so a scan replaces the list: favorites
/// first in their order, following their programme to its frequency here, then the rest by
/// frequency. Hidden stays hidden, and a programme keeps the name it was known by when the
/// scan heard none.
pub fn merged(own: &[FmStation], mut found: Vec<FmStation>) -> Vec<FmStation> {
    for st in found.iter_mut() {
        if st.name.is_empty()
            && let Some(old) = st
                .pi
                .and_then(|pi| own.iter().find(|o| o.pi == Some(pi) && !o.name.is_empty()))
        {
            st.name = old.name.clone();
        }
        st.hidden = own.iter().any(|o| o.khz == st.khz && o.hidden);
    }
    let mut out: Vec<FmStation> = Vec::new();
    for old in own.iter().filter(|o| o.favorite) {
        let followed = old
            .pi
            .and_then(|pi| found.iter().position(|f| f.pi == Some(pi)));
        let at = followed.or_else(|| found.iter().position(|f| f.khz == old.khz));
        let mut st = match at {
            Some(at) => found.remove(at),
            None => old.clone(),
        };
        st.favorite = true;
        st.hidden = old.hidden;
        if !out.iter().any(|o| o.khz == st.khz) {
            out.push(st);
        }
    }
    found.retain(|f| !out.iter().any(|o| o.khz == f.khz));
    found.sort_by_key(|s| s.khz);
    out.extend(found);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(khz: u32, pi: Option<u16>) -> FmStation {
        FmStation {
            khz,
            name: String::new(),
            pi,
            favorite: false,
            hidden: false,
        }
    }

    fn named(khz: u32, pi: u16, name: &str) -> FmStation {
        FmStation {
            name: name.into(),
            ..station(khz, Some(pi))
        }
    }

    fn sig(noise_db: f32, offset_hz: f32, stereo: bool) -> Signal {
        Signal {
            noise_db,
            offset_hz,
            stereo,
            ..Signal::default()
        }
    }

    #[test]
    fn the_band_runs_in_tenth_megahertz_steps() {
        assert_eq!(count(), 206);
        assert_eq!(khz_at(0), 87_500);
        assert_eq!(khz_at(count() - 1), 108_000);
        assert_eq!(mhz(98_100), "98.1");
        assert_eq!(mhz(108_000), "108.0");
        assert!(in_band(90_300));
        assert!(!in_band(90_350));
        assert!(!in_band(87_400));
        assert!(!in_band(108_100));
    }

    #[test]
    fn a_seek_runs_round_the_band() {
        assert_eq!(seek_next(98_100, true), 98_200);
        assert_eq!(seek_next(98_100, false), 98_000);
        assert_eq!(seek_next(108_000, true), 87_500);
        assert_eq!(seek_next(87_500, false), 108_000);
    }

    fn probe(signals: &[(Signal, bool)]) -> Vec<Verdict> {
        let start = Instant::now();
        let mut p = Probe::default();
        signals
            .iter()
            .enumerate()
            .map(|(i, (s, synced))| {
                p.step(s, *synced, start + Duration::from_millis(i as u64 * 100))
            })
            .collect()
    }

    #[test]
    fn noise_is_left_after_the_measurement() {
        let v = probe(&[(sig(-6.0, 0.0, false), false); MEASURE_BLOCKS]);
        assert_eq!(v.last(), Some(&Verdict::Nothing));
        assert!(v[..MEASURE_BLOCKS - 1].iter().all(|v| *v == Verdict::Wait));
        let beside = probe(&[(sig(-30.0, -95_000.0, false), false); MEASURE_BLOCKS]);
        assert_eq!(
            beside.last(),
            Some(&Verdict::Nothing),
            "the neighbour 100 kHz away"
        );
    }

    #[test]
    fn a_weak_station_counts_by_its_pilot() {
        let quiet = sig(-17.0, 200.0, false);
        let locked = sig(-17.0, 200.0, true);
        let mut s = vec![(quiet, false); MEASURE_BLOCKS + 3];
        s.extend(vec![(locked, false); 20]);
        let v = probe(&s);
        let at = v.iter().position(|v| *v != Verdict::Wait);
        assert_eq!(v[at.unwrap()], Verdict::Station);
    }

    #[test]
    fn a_candidate_without_pilot_or_rds_is_nothing() {
        let v = probe(&vec![(sig(-14.0, 500.0, false), false); 40]);
        let at = v.iter().position(|v| *v != Verdict::Wait).unwrap();
        assert_eq!(v[at], Verdict::Nothing);
        assert!(
            at * 100 <= 1000 + MEASURE_BLOCKS * 100,
            "gives up after a second"
        );
    }

    #[test]
    fn rds_keeps_the_listen_going_for_the_name() {
        let ms = Duration::from_millis;
        assert_eq!(
            verdict(ms(1500), true, false),
            Verdict::Station,
            "stereo, no RDS"
        );
        assert_eq!(verdict(ms(1500), true, true), Verdict::Wait);
        assert_eq!(verdict(ms(2500), true, true), Verdict::Station);
        assert_eq!(
            verdict(ms(2500), false, true),
            Verdict::Station,
            "RDS without pilot"
        );
        assert_eq!(verdict(ms(500), false, false), Verdict::Wait);
    }

    #[test]
    fn a_gain_change_restarts_only_a_measurement() {
        let mut p = Probe::default();
        let now = Instant::now();
        p.step(&sig(-20.0, 0.0, false), false, now);
        assert!(p.restart());
        for _ in 0..MEASURE_BLOCKS {
            p.step(&sig(-20.0, 0.0, false), false, now);
        }
        assert!(!p.measuring());
        assert!(!p.restart(), "the listen goes on");
        assert!(!p.shows_station());
        p.step(&sig(-20.0, 0.0, true), false, now);
        assert!(p.shows_station());
    }

    #[test]
    fn a_programme_on_two_frequencies_stays_on_the_cleaner_one() {
        let mut found = Vec::new();
        keep_found(&mut found, station(89_000, Some(0xd3c2)), -25.0);
        keep_found(&mut found, station(90_300, None), -30.0);
        keep_found(&mut found, station(91_700, None), -30.0);
        keep_found(&mut found, station(97_100, Some(0xd3c2)), -38.0);
        keep_found(&mut found, station(99_200, Some(0xd3c2)), -31.0);
        let khz: Vec<u32> = found.iter().map(|(s, _)| s.khz).collect();
        assert_eq!(khz, [97_100, 90_300, 91_700]);
    }

    #[test]
    fn a_scan_elsewhere_replaces_all_but_the_favorites() {
        let own = vec![
            FmStation {
                favorite: true,
                ..named(103_600, 0xd3a1, "RADIO HH")
            },
            named(106_800, 0xd3e0, "ROCK HH"),
            FmStation {
                hidden: true,
                ..named(90_300, 0xd3c2, "NDR 90,3")
            },
            named(88_700, 0xd3d0, "GONE"),
        ];
        let found = vec![
            station(102_900, Some(0xd3a1)),
            station(106_800, Some(0xd3e0)),
            station(90_300, Some(0xd3c2)),
            named(89_500, 0xd3c3, "NDR 1 SH"),
            station(103_600, Some(0x1234)),
        ];
        let list = merged(&own, found);
        let khz: Vec<u32> = list.iter().map(|s| s.khz).collect();
        assert_eq!(khz, [102_900, 89_500, 90_300, 103_600, 106_800]);
        assert!(list[0].favorite, "the favorite followed its programme");
        assert_eq!(list[0].name, "RADIO HH", "and kept its name");
        assert!(list[2].hidden, "hidden stays hidden");
        assert_eq!(list[2].name, "NDR 90,3");
        assert_eq!(list[3].name, "", "another programme on the old frequency");
        assert!(!list[4].favorite);
    }

    #[test]
    fn a_favorite_not_heard_stays() {
        let own = vec![FmStation {
            favorite: true,
            ..named(94_200, 0x1111, "FAR")
        }];
        let list = merged(&own, vec![station(99_200, Some(0x2222))]);
        let khz: Vec<u32> = list.iter().map(|s| s.khz).collect();
        assert_eq!(khz, [94_200, 99_200]);
        let list = merged(&own, vec![station(94_200, Some(0x2222))]);
        assert_eq!(list.len(), 1, "one entry per frequency");
    }

    #[test]
    fn quiet_means_quality() {
        let q = |noise_db, rds| {
            signal(&Signal {
                noise_db,
                rds,
                ..Signal::default()
            })
        };
        assert_eq!(q(0.0, 0).quality, 0);
        assert_eq!(q(-20.0, 0).quality, 50);
        assert_eq!(q(-60.0, 0).quality, 100);
        assert!(q(-40.0, 90).rds);
        assert!(!q(-40.0, 10).rds);
    }
}
