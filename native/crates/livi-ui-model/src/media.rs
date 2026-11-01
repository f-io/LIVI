// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::settings::color::{Hsl, from_hsl};

const STALL: Duration = Duration::from_millis(3000);
const OPTIMISTIC: Duration = Duration::from_millis(4000);
const JUMP_BACK_MS: f64 = 500.0;

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
        self.live = elapsed_ms.unwrap_or(0.0);
        self.at = Some(now);
        self.guess.settle(playing);
    }

    pub fn toggled(&mut self, now: Instant) {
        let real = self.real_playing();
        self.guess.toggle(real, now);
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
    pub service_id: u32,
    pub number: usize,
    pub label: String,
    pub favorite: bool,
    pub tuned: bool,
    pub logo: Option<String>,
    pub initials: String,
    pub badge: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dab {
    pub receiver: bool,
    pub active: bool,
    pub playing: bool,
    pub title: Option<String>,
    pub text: String,
    pub ensemble: String,
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

pub fn dab(state: &Value) -> Dab {
    let d = &state["dab"];
    let tuned = d["station"].as_u64().map(|v| v as u32);
    let mut out = Dab {
        receiver: !d["receiver"].is_null(),
        active: d["active"].as_bool().unwrap_or(false),
        playing: d["playing"].as_bool().unwrap_or(false),
        text: d["text"].as_str().unwrap_or("").to_string(),
        tuned,
        ..Dab::default()
    };
    let signal = &d["signal"];
    if signal["locked"].as_bool() == Some(true) {
        let q = signal["quality"].as_u64().unwrap_or(0);
        out.signal_bars = if q >= 80 {
            3
        } else if q >= 40 {
            2
        } else {
            1
        };
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
                service_id: id,
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
}
