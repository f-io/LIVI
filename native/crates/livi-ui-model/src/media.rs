// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::settings::color::{Hsl, from_hsl};

const STALL: Duration = Duration::from_millis(3000);
const OPTIMISTIC: Duration = Duration::from_millis(4000);
const JUMP_BACK_MS: f64 = 500.0;
const SEEK_HOLD: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub elapsed_ms: f64,
    pub progress: f32,
    pub playing: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Guess {
    wanted: Option<(bool, Instant)>,
}

impl Guess {
    pub fn shown(&self, real: bool, now: Instant) -> bool {
        match self.wanted {
            Some((want, until)) if now < until => want,
            _ => real,
        }
    }

    pub fn toggle(&mut self, real: bool, now: Instant) {
        self.wanted = Some((!self.shown(real, now), now + OPTIMISTIC));
    }

    pub fn pending(&self, now: Instant) -> bool {
        self.wanted.is_some_and(|(_, until)| now < until)
    }

    pub fn settle(&mut self, real: bool) {
        if self.wanted.is_some_and(|(want, _)| want == real) {
            self.wanted = None;
        }
    }
}

#[derive(Debug, Default)]
pub struct Playback {
    reported: Option<f64>,
    duration: f64,
    playing: bool,
    live: f64,
    at: Option<Instant>,
    last_progress: Option<Instant>,
    stalled: bool,
    guess: Guess,
    back_once: bool,
    last_fraction: f32,
    last_elapsed: f64,
    track: String,
    seek_until: Option<Instant>,
}

impl Playback {
    pub fn update(
        &mut self,
        track: &str,
        duration_ms: f64,
        elapsed_ms: Option<f64>,
        playing: bool,
        now: Instant,
    ) {
        if track != self.track {
            self.track = track.to_string();
            self.last_fraction = 0.0;
            self.last_elapsed = 0.0;
        }
        let progressed = elapsed_ms.is_some() && elapsed_ms != self.reported;
        if progressed {
            self.reported = elapsed_ms;
        }
        let resumed = playing && !self.playing;
        self.playing = playing;
        self.duration = duration_ms.max(0.0);
        if progressed || resumed || self.last_progress.is_none() {
            self.last_progress = Some(now);
            self.stalled = false;
        }
        if self.seek_until.is_none_or(|until| now >= until) {
            self.live = elapsed_ms.unwrap_or(0.0);
            self.at = Some(now);
        }
        self.guess.settle(playing);
    }

    pub fn toggled(&mut self, now: Instant) {
        let real = self.real_playing();
        self.guess.toggle(real, now);
    }

    pub fn seek(&mut self, position_ms: f64, now: Instant) {
        self.live = position_ms.clamp(0.0, self.duration);
        self.at = Some(now);
        self.back_once = true;
        self.seek_until = Some(now + SEEK_HOLD);
    }

    pub fn went_back(&mut self) {
        self.back_once = true;
    }

    pub fn reset(&mut self) {
        self.guess = Guess::default();
        self.back_once = false;
        self.last_fraction = 0.0;
    }

    fn real_playing(&self) -> bool {
        self.playing && !self.stalled
    }

    fn shown_playing(&self, now: Instant) -> bool {
        self.guess.shown(self.real_playing(), now)
    }

    pub fn tick(&mut self, now: Instant) -> Shown {
        let dt = self.at.map_or(0.0, |at| {
            now.saturating_duration_since(at).as_secs_f64() * 1000.0
        });
        self.at = Some(now);
        if self.playing {
            let since = self
                .last_progress
                .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
            if since > STALL {
                self.live = self.reported.unwrap_or(self.live);
                self.stalled = true;
            } else {
                self.live = (self.live + dt).clamp(0.0, self.duration.max(0.0));
            }
        }
        let elapsed = self.live.max(0.0);
        let mut fraction = if self.duration > 0.0 {
            (elapsed / self.duration) as f32
        } else {
            0.0
        };
        let restart = self.back_once || self.last_elapsed - elapsed > JUMP_BACK_MS;
        if self.real_playing() && !restart && fraction + 0.001 < self.last_fraction {
            fraction = self.last_fraction;
        }
        fraction = fraction.clamp(0.0, 1.0);
        self.last_fraction = fraction;
        self.last_elapsed = elapsed;
        self.back_once = false;
        Shown {
            elapsed_ms: elapsed,
            progress: fraction,
            playing: self.shown_playing(now),
        }
    }

    pub fn running(&self) -> bool {
        self.real_playing()
    }

    pub fn guessing(&self, now: Instant) -> bool {
        self.guess.pending(now)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub id: u32,
    pub number: usize,
    pub label: String,
    pub favorite: bool,
    pub tuned: bool,
    pub logo: Option<String>,
    pub initials: String,
    pub badge: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Radio {
    pub receiver: bool,
    pub active: bool,
    pub playing: bool,
    pub title: Option<String>,
    pub text: String,
    pub ensemble: String,
    pub stereo: bool,
    pub signal_bars: u8,
    pub tuned: Option<u32>,
    pub tuned_favorite: bool,
    pub tuned_logo: Option<String>,
    pub tuned_initials: String,
    pub tuned_badge: [u8; 3],
    pub stations: Vec<Station>,
    pub tuned_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    Play,
    Next,
    Prev,
}

pub fn pressed(binding: &str) -> Option<Pressed> {
    match binding {
        "playPause" | "play" | "pause" | "stop" => Some(Pressed::Play),
        "next" => Some(Pressed::Next),
        "prev" => Some(Pressed::Prev),
        _ => None,
    }
}

pub fn initials(label: &str) -> String {
    let words: Vec<&str> = label
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'))
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = if words.len() > 1 {
        words[0]
            .chars()
            .take(1)
            .chain(words[1].chars().take(1))
            .collect()
    } else {
        words
            .first()
            .map_or("?".into(), |w| w.chars().take(2).collect())
    };
    letters.to_uppercase()
}

pub fn hue(label: &str) -> u32 {
    label
        .encode_utf16()
        .fold(0u32, |h, c| (h * 31 + u32::from(c)) % 360)
}

pub fn badge(label: &str) -> [u8; 3] {
    from_hsl(Hsl {
        h: f64::from(hue(label)),
        s: 45.0,
        l: 38.0,
    })
}

fn signal_bars(quality: u64) -> u8 {
    if quality >= 80 {
        3
    } else if quality >= 40 {
        2
    } else {
        1
    }
}

pub fn mhz(khz: u32) -> String {
    format!("{}.{}", khz / 1000, khz % 1000 / 100)
}

pub fn dab(state: &Value) -> Radio {
    let d = &state["dab"];
    let tuned = d["station"].as_u64().map(|v| v as u32);
    let mut out = Radio {
        receiver: !d["receiver"].is_null(),
        active: d["active"].as_bool().unwrap_or(false),
        playing: d["playing"].as_bool().unwrap_or(false),
        text: d["text"].as_str().unwrap_or("").to_string(),
        tuned,
        ..Radio::default()
    };
    let signal = &d["signal"];
    if signal["locked"].as_bool() == Some(true) {
        out.signal_bars = signal_bars(signal["quality"].as_u64().unwrap_or(0));
    }
    let all = d["stations"].as_array().cloned().unwrap_or_default();
    for st in &all {
        let id = st["serviceId"].as_u64().unwrap_or(0) as u32;
        let label = st["label"].as_str().unwrap_or("").to_string();
        if Some(id) == tuned {
            out.title = Some(label.clone());
            out.ensemble = format!(
                "{} · {}",
                st["ensemble"].as_str().unwrap_or(""),
                st["channel"].as_str().unwrap_or("")
            );
            out.tuned_favorite = st["favorite"].as_bool().unwrap_or(false);
            out.tuned_logo = st["logo"].as_str().map(str::to_string);
        }
    }
    let shown_label = out.title.clone().unwrap_or_else(|| "DAB+".into());
    out.tuned_initials = initials(&shown_label);
    out.tuned_badge = badge(&shown_label);
    out.stations = all
        .iter()
        .filter(|st| st["hidden"].as_bool() != Some(true))
        .enumerate()
        .map(|(i, st)| {
            let id = st["serviceId"].as_u64().unwrap_or(0) as u32;
            let label = st["label"].as_str().unwrap_or("");
            let short = st["shortLabel"].as_str().unwrap_or("");
            Station {
                id,
                number: i + 1,
                label: if short.is_empty() { label } else { short }.to_string(),
                favorite: st["favorite"].as_bool().unwrap_or(false),
                tuned: Some(id) == tuned,
                logo: st["logo"].as_str().map(str::to_string),
                initials: initials(label),
                badge: badge(label),
            }
        })
        .collect();
    out.tuned_index = out.stations.iter().position(|s| s.tuned);
    out
}

fn fm_name(st: &Value) -> Option<String> {
    let name = st["name"].as_str().unwrap_or("").trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub fn fm(state: &Value) -> Radio {
    let f = &state["fm"];
    let tuned = f["station"].as_u64().map(|v| v as u32);
    let all = f["stations"].as_array().cloned().unwrap_or_default();
    let khz_of = |st: &Value| st["khz"].as_u64().unwrap_or(0) as u32;
    let mut out = Radio {
        receiver: !state["dab"]["receiver"].is_null(),
        active: f["active"].as_bool().unwrap_or(false),
        playing: f["playing"].as_bool().unwrap_or(false),
        text: f["text"].as_str().unwrap_or("").to_string(),
        tuned,
        ..Radio::default()
    };
    let signal = &f["signal"];
    let quality = signal["quality"].as_u64().unwrap_or(0);
    if out.active && quality > 0 {
        out.signal_bars = signal_bars(quality);
    }
    let mut name = None;
    if let Some(khz) = tuned {
        let listed = all.iter().find(|st| khz_of(st) == khz);
        name = f["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .or_else(|| listed.and_then(fm_name));
        out.title = Some(name.clone().unwrap_or_else(|| format!("{} MHz", mhz(khz))));
        out.ensemble = format!("{} MHz", mhz(khz));
        out.stereo = out.active && signal["stereo"].as_bool() == Some(true);
        out.tuned_favorite = listed.is_some_and(|st| st["favorite"].as_bool() == Some(true));
    }
    let shown = name.or_else(|| tuned.map(mhz));
    out.tuned_initials = shown.as_deref().map_or("FM".into(), fm_initials);
    out.tuned_badge = badge(shown.as_deref().unwrap_or("FM"));
    out.stations = all
        .iter()
        .filter(|st| st["hidden"].as_bool() != Some(true))
        .enumerate()
        .map(|(i, st)| {
            let khz = khz_of(st);
            let label = fm_name(st).unwrap_or_else(|| mhz(khz));
            Station {
                id: khz,
                number: i + 1,
                favorite: st["favorite"].as_bool().unwrap_or(false),
                tuned: Some(khz) == tuned,
                logo: None,
                initials: fm_initials(&label),
                badge: badge(&label),
                label,
            }
        })
        .collect();
    out.tuned_index = out.stations.iter().position(|s| s.tuned);
    out
}

/// A station without a name shows its frequency on the badge.
fn fm_initials(label: &str) -> String {
    if label.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        label.to_string()
    } else {
        initials(label)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn time_runs_while_playing_and_stops_on_a_stall() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.update("a", 60_000.0, Some(1_000.0), true, t0);
        let s = p.tick(at(t0, 1_000));
        assert!((s.elapsed_ms - 2_000.0).abs() < 1.0);
        assert!(s.playing);
        let s = p.tick(at(t0, 3_500));
        assert!(!s.playing, "no progress for 3 s is a stall");
        assert!((s.elapsed_ms - 1_000.0).abs() < 1.0);
    }

    #[test]
    fn the_bar_ignores_small_jitter_but_follows_a_jump_back() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.update("a", 100_000.0, Some(50_000.0), true, t0);
        let first = p.tick(t0).progress;
        p.update("a", 100_000.0, Some(49_800.0), true, at(t0, 10));
        assert_eq!(p.tick(at(t0, 10)).progress, first);
        p.update("a", 100_000.0, Some(10_000.0), true, at(t0, 20));
        assert!(p.tick(at(t0, 20)).progress < first);
    }

    #[test]
    fn a_seek_holds_its_target_until_the_phone_caught_up() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.update("a", 100_000.0, Some(50_000.0), true, t0);
        p.tick(t0);
        p.seek(20_000.0, at(t0, 100));
        p.update("a", 100_000.0, Some(50_200.0), true, at(t0, 300));
        assert!((p.tick(at(t0, 300)).elapsed_ms - 20_200.0).abs() < 1.0);
        p.update("a", 100_000.0, Some(21_000.0), true, at(t0, 1_200));
        assert!((p.tick(at(t0, 1_200)).elapsed_ms - 21_000.0).abs() < 1.0);
    }

    #[test]
    fn a_tap_shows_the_new_state_until_the_phone_agrees() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.update("a", 100_000.0, Some(0.0), true, t0);
        p.toggled(t0);
        assert!(!p.tick(at(t0, 10)).playing);
        p.update("a", 100_000.0, Some(4_000.0), true, at(t0, 4_000));
        assert!(
            p.tick(at(t0, 4_100)).playing,
            "the guess times out after 4 s"
        );
        p.toggled(at(t0, 4_200));
        p.update("a", 100_000.0, Some(0.0), false, at(t0, 4_300));
        assert!(!p.tick(at(t0, 4_400)).playing);
    }

    #[test]
    fn media_bindings_flash_their_button() {
        assert_eq!(pressed("pause"), Some(Pressed::Play));
        assert_eq!(pressed("prev"), Some(Pressed::Prev));
        assert_eq!(pressed("home"), None);
    }

    #[test]
    fn station_badges() {
        assert_eq!(initials("Radio Eins"), "RE");
        assert_eq!(initials("bayern3"), "BA");
        assert_eq!(initials(""), "?");
        assert_eq!(hue("abc"), hue("abc"));
        assert!(hue("abc") < 360);
    }

    #[test]
    fn dab_view_from_state() {
        let state = json!({ "dab": {
            "receiver": "rtl-sdr", "active": true, "station": 2, "playing": true,
            "text": "Now on air", "signal": { "locked": true, "quality": 55 },
            "stations": [
                { "serviceId": 1, "label": "Hidden FM", "shortLabel": "", "ensemble": "E", "channel": "5C", "hidden": true },
                { "serviceId": 2, "label": "Radio Eins", "shortLabel": "Eins", "ensemble": "Berlin", "channel": "7D", "favorite": true }
            ]
        } });
        let d = dab(&state);
        assert!(d.receiver && d.active);
        assert_eq!(d.title.as_deref(), Some("Radio Eins"));
        assert_eq!(d.ensemble, "Berlin · 7D");
        assert_eq!(d.signal_bars, 2);
        assert_eq!(d.stations.len(), 1);
        assert_eq!(d.stations[0].label, "Eins");
        assert_eq!(d.tuned_index, Some(0));
    }

    #[test]
    fn fm_view_from_state() {
        let state = json!({
            "dab": { "receiver": "rtl-sdr" },
            "fm": {
                "active": true, "station": 98100, "playing": true, "name": "NDR 2",
                "text": "Moin", "signal": { "quality": 90, "stereo": true, "rds": true },
                "stations": [
                    { "khz": 89000, "name": "", "hidden": false },
                    { "khz": 91700, "name": "gone", "hidden": true },
                    { "khz": 98100, "name": "N2", "favorite": true }
                ]
            }
        });
        let f = fm(&state);
        assert!(f.receiver && f.active && f.stereo);
        assert_eq!(f.title.as_deref(), Some("NDR 2"), "RDS names it on air");
        assert_eq!(f.ensemble, "98.1 MHz");
        assert_eq!(f.signal_bars, 3);
        assert!(f.tuned_favorite);
        let labels: Vec<&str> = f.stations.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["89.0", "N2"]);
        assert_eq!(f.stations[0].initials, "89.0");
        assert_eq!(f.tuned_index, Some(1));
    }

    #[test]
    fn a_manually_tuned_frequency_is_its_own_title() {
        let state = json!({ "fm": { "active": true, "station": 104000, "stations": [] } });
        let f = fm(&state);
        assert!(!f.receiver);
        assert_eq!(f.title.as_deref(), Some("104.0 MHz"));
        assert_eq!(f.tuned_initials, "104.0");
        assert!(!f.tuned_favorite && f.tuned_index.is_none());
        assert_eq!(fm(&json!({})).tuned_initials, "FM");
    }
}
