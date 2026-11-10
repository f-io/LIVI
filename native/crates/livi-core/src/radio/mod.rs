// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod audio;
mod dab;
mod fm;
mod focus;
mod list;
mod thread;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use base64::Engine;
use livi_core_proto::message::MediaControl;
use livi_core_proto::state::{Dab, DabScan, DabStation, Fm, FmStation, Protocol, State};
use livi_dab::CHANNELS;
use livi_fm::Rds;
use livi_media::gst_host::GstHost;
use tokio::sync::{mpsc, watch};

use crate::hub::Hub;
use crate::state_files::{Sdr, load_sdr, save_sdr};
use crate::status_file::Activity;
use audio::Audio;
use focus::{Focus, Turn};
use list::{hidden_last, merged, move_station, star, step_target};
use thread::{Cmd, Radio};

/// One stick, so one band at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Dab,
    Fm,
}

/// A DAB+ station by its service id, an FM one by its frequency in kHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Station {
    pub band: Band,
    pub id: u32,
}

impl Station {
    pub fn dab(service: u32) -> Self {
        Self {
            band: Band::Dab,
            id: service,
        }
    }

    pub fn fm(khz: u32) -> Self {
        Self {
            band: Band::Fm,
            id: khz,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RadioAsk {
    Scan(Band),
    StopScan,
    Play(Station),
    /// Without a band, the one heard last.
    Resume(Option<Band>),
    Pause,
    Stop,
    Step(i32),
    /// To the next FM station up or down the band.
    Seek {
        up: bool,
    },
    Favorite {
        station: Station,
        favorite: bool,
    },
    Hide {
        station: Station,
        hidden: bool,
    },
    Move {
        station: Station,
        by: i32,
    },
    Duck {
        level: f64,
        ramp_ms: u32,
    },
}

pub type RadioAsks = mpsc::UnboundedReceiver<RadioAsk>;

pub fn media_ask(
    dab: &Dab,
    fm: &Fm,
    phone: Option<Protocol>,
    control: MediaControl,
) -> Option<RadioAsk> {
    dab.receiver.as_ref()?;
    let playing = dab.playing || fm.playing;
    let stations = !dab.stations.is_empty() || !fm.stations.is_empty();
    let take = match phone {
        Some(Protocol::Carplay) => false,
        Some(Protocol::Androidauto | Protocol::Airplay) => playing,
        None => dab.active || fm.active || stations,
    };
    if !take {
        return None;
    }
    Some(match control {
        MediaControl::Next => RadioAsk::Step(1),
        MediaControl::Prev => RadioAsk::Step(-1),
        MediaControl::Pause => RadioAsk::Pause,
        MediaControl::PlayPause if playing => RadioAsk::Pause,
        MediaControl::PlayPause | MediaControl::Play => RadioAsk::Resume(None),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Tuning {
    gain: Option<i32>,
    ppm: i32,
    deemphasis_us: u32,
}

fn tuning(state: &State) -> Tuning {
    let c = &state.config;
    let tenths = (c.dab_gain_db * 10.0).round() as i32;
    let gain = (!c.dab_gain_auto).then(|| nearest_gain(tenths));
    Tuning {
        gain,
        ppm: c.dab_ppm,
        deemphasis_us: c.fm_deemphasis_us,
    }
}

fn nearest_gain(tenths: i32) -> i32 {
    livi_rtlsdr::gains()
        .into_iter()
        .min_by_key(|g| (g - tenths).abs())
        .unwrap_or(0)
}

#[derive(Default)]
struct Found {
    dab: Vec<DabStation>,
    fm: Vec<(FmStation, f32)>,
}

impl Found {
    fn count(&self, band: Band) -> u32 {
        match band {
            Band::Dab => self.dab.len() as u32,
            Band::Fm => self.fm.len() as u32,
        }
    }
}

pub async fn run(
    hub: Arc<Hub>,
    gst: GstHost,
    mut asks: RadioAsks,
    mut activity: watch::Receiver<Activity>,
    user_data: PathBuf,
    resume: Option<Band>,
) {
    let (cmd_tx, cmd_rx) = std_mpsc::channel();
    let (ev_tx, mut events) = mpsc::unbounded_channel();
    let mut out = Out {
        hub: hub.clone(),
        gst: gst.clone(),
        cmds: cmd_tx.clone(),
        user_data: user_data.clone(),
        own: load_sdr(&user_data),
        logos: dab::load_images(&user_data.join(dab::LOGOS)),
    };
    hidden_last(&mut out.own.stations);
    hidden_last(&mut out.own.fm_stations);
    let gains_db = livi_rtlsdr::gains()
        .into_iter()
        .map(|g| g as f64 / 10.0)
        .collect();
    hub.update(|s| s.dab.gains_db = gains_db);
    out.publish();
    let mut state: watch::Receiver<State> = hub.watch();
    let mut tune = tuning(&state.borrow());
    let started = std::thread::Builder::new()
        .name("radio".into())
        .spawn(move || thread::run(cmd_rx, ev_tx, tune));
    if let Err(e) = started {
        eprintln!("[radio] no radio thread: {e}");
        return;
    }
    let mut audio = Audio::default();
    let mut found = Found::default();
    let mut band = resume.unwrap_or(Band::Dab);
    let mut resume = resume.is_some();
    let mut focus = Focus::default();

    loop {
        tokio::select! {
            ask = asks.recv() => {
                let Some(ask) = ask else { return };
                let s = state.borrow().clone();
                if !matches!(
                    ask,
                    RadioAsk::Duck { .. } | RadioAsk::Favorite { .. } | RadioAsk::Hide { .. } | RadioAsk::Move { .. }
                ) {
                    focus.let_go();
                }
                match ask {
                    RadioAsk::Scan(b) => {
                        band = b;
                        found = Found::default();
                        audio.close(&gst);
                        let total = match b {
                            Band::Dab => CHANNELS.len(),
                            Band::Fm => fm::count(),
                        } as u32;
                        hub.update(|s| {
                            quiet(&mut s.dab, &mut s.fm);
                            let scan = Some(DabScan { total, ..Default::default() });
                            match b {
                                Band::Dab => s.dab.scan = scan,
                                Band::Fm => s.fm.scan = scan,
                            }
                        });
                        let _ = cmd_tx.send(Cmd::Scan(b));
                    }
                    RadioAsk::StopScan | RadioAsk::Stop => {
                        audio.close(&gst);
                        let _ = cmd_tx.send(Cmd::Stop);
                        hub.update(|s| quiet(&mut s.dab, &mut s.fm));
                    }
                    RadioAsk::Pause => out.pause(&mut audio),
                    RadioAsk::Duck { level, ramp_ms } => audio.duck(&gst, level, ramp_ms),
                    RadioAsk::Play(station) => {
                        band = station.band;
                        out.play(&mut audio, &s, station);
                    }
                    RadioAsk::Resume(b) => {
                        band = b.unwrap_or(band);
                        out.resume(&mut audio, &s, band);
                    }
                    RadioAsk::Seek { up } => {
                        band = Band::Fm;
                        out.seek(&mut audio, &s, up);
                    }
                    RadioAsk::Favorite { station, favorite } => out.favorite(&s, station, favorite),
                    RadioAsk::Hide { station, hidden } => out.hide(station, hidden),
                    RadioAsk::Move { station, by } => out.move_station(station, by),
                    RadioAsk::Step(by) => {
                        let next = match band {
                            Band::Dab => step_target(&s.dab.stations, s.dab.station, by).map(Station::dab),
                            Band::Fm => step_target(&s.fm.stations, s.fm.station, by).map(Station::fm),
                        };
                        if let Some(station) = next {
                            out.play(&mut audio, &s, station);
                        }
                    }
                }
            }
            ev = events.recv() => {
                let Some(ev) = ev else { return };
                match ev {
                    Radio::Receiver(None) => {
                        audio.close(&gst);
                        found = Found::default();
                        let _ = cmd_tx.send(Cmd::Stop);
                        hub.update(|s| stick_gone(&mut s.dab, &mut s.fm));
                    }
                    Radio::Receiver(name) => {
                        hub.update(|s| s.dab.receiver = name);
                        if std::mem::take(&mut resume) {
                            let s = state.borrow().clone();
                            out.resume(&mut audio, &s, band);
                        }
                    }
                    Radio::Progress(b, p) => {
                        let scan = Some(DabScan { found: found.count(b), ..p });
                        hub.update(|s| match b {
                            Band::Dab if s.dab.scan.is_some() => s.dab.scan = scan,
                            Band::Fm if s.fm.scan.is_some() => s.fm.scan = scan,
                            _ => {}
                        });
                    }
                    Radio::DabFound(list) => {
                        for st in list {
                            if !found.dab.iter().any(|f| f.service_id == st.service_id) {
                                found.dab.push(st);
                            }
                        }
                        let n = found.count(Band::Dab);
                        hub.update(|s| if let Some(p) = s.dab.scan.as_mut() { p.found = n });
                    }
                    Radio::FmFound { station, noise_db } => {
                        fm::keep_found(&mut found.fm, station, noise_db);
                        let n = found.count(Band::Fm);
                        hub.update(|s| if let Some(p) = s.fm.scan.as_mut() { p.found = n });
                    }
                    // A scan cut short by a lost stick leaves the list as it was.
                    Radio::ScanDone(Band::Dab) => {
                        let list = std::mem::take(&mut found.dab);
                        if state.borrow().dab.scan.is_some() {
                            out.own.stations = merged(&out.own.stations, list, |s| s.label.to_lowercase());
                            out.keep();
                        }
                        hub.update(|s| s.dab.scan = None);
                    }
                    Radio::ScanDone(Band::Fm) => {
                        let list = std::mem::take(&mut found.fm).into_iter().map(|(st, _)| st).collect();
                        if state.borrow().fm.scan.is_some() {
                            out.own.fm_stations = fm::merged(&out.own.fm_stations, list);
                            out.keep();
                        }
                        hub.update(|s| s.fm.scan = None);
                    }
                    Radio::FmSeeking(khz) => hub.update(|s| tuned_fm(&mut s.fm, khz)),
                    Radio::FmTuned(khz) => {
                        out.keep_last_fm(khz);
                        hub.update(|s| tuned_fm(&mut s.fm, khz));
                    }
                    Radio::DabSignal(sig) => hub.update(|s| s.dab.signal = sig),
                    Radio::FmSignal(sig) => hub.update(|s| s.fm.signal = fm::signal(&sig)),
                    Radio::Format(b, format) => {
                        let s = state.borrow().clone();
                        let active = match b {
                            Band::Dab => s.dab.active,
                            Band::Fm => s.fm.active,
                        };
                        if active {
                            audio.open(&gst, &s, format).await;
                        }
                        let playing = audio.stream.is_some();
                        hub.update(|s| match b {
                            Band::Dab => s.dab.playing = playing && s.dab.active,
                            Band::Fm => s.fm.playing = playing && s.fm.active,
                        });
                    }
                    Radio::Audio(units) => audio.push(&gst, &units),
                    Radio::DabText(t) => hub.update(|s| if s.dab.active { s.dab.text = Some(t) }),
                    Radio::Rds(r) => {
                        if let Rds::Name(name) = &r {
                            let tuned = {
                                let s = state.borrow();
                                s.fm.station.filter(|_| s.fm.active)
                            };
                            out.rename_fm(tuned, name);
                        }
                        hub.update(|s| rds(&mut s.fm, r));
                    }
                    Radio::Logo { service, mime, data } => {
                        dab::keep_image(&user_data.join(dab::LOGOS), service, mime, &data);
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        if out.logos.insert(service, b64.clone()) != Some(b64) {
                            out.publish();
                        }
                    }
                    Radio::Error(e) => {
                        eprintln!("[radio] {e}");
                        audio.close(&gst);
                        hub.update(|s| match band {
                            Band::Dab => {
                                s.dab.error = Some(e);
                                s.dab.playing = false;
                            }
                            Band::Fm => {
                                s.fm.error = Some(e);
                                s.fm.playing = false;
                            }
                        });
                    }
                }
            }
            changed = activity.changed() => {
                if changed.is_err() {
                    return;
                }
                let now = *activity.borrow_and_update();
                let s = state.borrow().clone();
                match focus.next(now, s.dab.playing || s.fm.playing) {
                    Some(Turn::Pause) => {
                        println!("[radio] paused for the phone");
                        out.pause(&mut audio);
                    }
                    Some(Turn::Resume) => {
                        println!("[radio] back after the phone");
                        out.resume(&mut audio, &s, band);
                    }
                    None => {}
                }
            }
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let s = state.borrow_and_update().clone();
                let next = tuning(&s);
                if next != tune {
                    tune = next;
                    let _ = cmd_tx.send(Cmd::Tuning(tune));
                }
                audio.follow(&gst, &s);
            }
        }
    }
}

fn rds(fm: &mut Fm, r: Rds) {
    if !fm.active {
        return;
    }
    match r {
        Rds::Name(n) => fm.name = Some(n.trim().to_string()).filter(|n| !n.is_empty()),
        Rds::Text(t) => fm.text = Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        Rds::Pi(_) | Rds::Pty(_) => {}
    }
}

fn tuned_fm(fm: &mut Fm, khz: u32) {
    if !fm.active {
        return;
    }
    fm.station = Some(khz);
    fm.name = None;
    fm.text = None;
    fm.signal = Default::default();
}

fn quiet_dab(dab: &mut Dab) {
    dab.scan = None;
    dab.active = false;
    dab.station = None;
    dab.playing = false;
    dab.text = None;
}

fn quiet_fm(fm: &mut Fm) {
    fm.scan = None;
    fm.active = false;
    fm.station = None;
    fm.playing = false;
    fm.name = None;
    fm.text = None;
    fm.signal = Default::default();
}

fn quiet(dab: &mut Dab, fm: &mut Fm) {
    quiet_dab(dab);
    quiet_fm(fm);
}

fn stick_gone(dab: &mut Dab, fm: &mut Fm) {
    *dab = Dab {
        stations: std::mem::take(&mut dab.stations),
        last_station: dab.last_station,
        gains_db: std::mem::take(&mut dab.gains_db),
        ..Dab::default()
    };
    *fm = Fm {
        stations: std::mem::take(&mut fm.stations),
        last_station: fm.last_station,
        ..Fm::default()
    };
}

struct Out {
    hub: Arc<Hub>,
    gst: GstHost,
    cmds: std_mpsc::Sender<Cmd>,
    user_data: PathBuf,
    own: Sdr,
    logos: HashMap<u32, String>,
}

impl Out {
    fn pause(&self, audio: &mut Audio) {
        audio.close(&self.gst);
        let _ = self.cmds.send(Cmd::Stop);
        self.hub.update(|s| {
            s.dab.playing = false;
            s.fm.playing = false;
        });
    }

    fn publish(&self) {
        let own = &self.own;
        let mut stations = own.stations.clone();
        for st in &mut stations {
            st.logo = self.logos.get(&st.service_id).cloned();
        }
        self.hub.update(|s| {
            s.dab.stations = stations;
            s.dab.last_station = own.last_station;
            s.fm.stations = own.fm_stations.clone();
            s.fm.last_station = own.last_fm;
        });
    }

    fn keep(&mut self) {
        hidden_last(&mut self.own.stations);
        hidden_last(&mut self.own.fm_stations);
        save_sdr(&self.user_data, &self.own);
        self.publish();
    }

    fn resume(&mut self, audio: &mut Audio, s: &State, band: Band) {
        let playing = match band {
            Band::Dab => s.dab.playing,
            Band::Fm => s.fm.playing,
        };
        if playing {
            return;
        }
        let pick = match band {
            Band::Dab => {
                let list = &s.dab.stations;
                let wanted = s.dab.station.or(s.dab.last_station);
                list.iter()
                    .find(|st| Some(st.service_id) == wanted)
                    .or(list.iter().find(|st| !st.hidden))
                    .map(|st| Station::dab(st.service_id))
            }
            Band::Fm => {
                s.fm.station
                    .or(s.fm.last_station)
                    .or(s.fm.stations.iter().find(|st| !st.hidden).map(|st| st.khz))
                    .map(Station::fm)
            }
        };
        if let Some(station) = pick {
            self.play(audio, s, station);
        }
    }

    fn play(&mut self, audio: &mut Audio, s: &State, station: Station) {
        match station.band {
            Band::Dab => self.play_dab(audio, s, station.id),
            Band::Fm => self.play_fm(audio, station.id),
        }
    }

    fn play_dab(&mut self, audio: &mut Audio, s: &State, service: u32) {
        let Some(st) = s.dab.stations.iter().find(|st| st.service_id == service) else {
            self.hub
                .update(|s| s.dab.error = Some("station not in the list, scan again".into()));
            return;
        };
        audio.close(&self.gst);
        let _ = self.cmds.send(Cmd::Play {
            frequency: st.frequency_hz,
            service,
        });
        if self.own.last_station != Some(service) {
            self.own.last_station = Some(service);
            self.keep();
        }
        self.hub.update(|s| {
            quiet(&mut s.dab, &mut s.fm);
            s.dab.active = true;
            s.dab.station = Some(service);
            s.dab.error = None;
        });
    }

    fn play_fm(&mut self, audio: &mut Audio, khz: u32) {
        if !fm::in_band(khz) {
            self.hub
                .update(|s| s.fm.error = Some(format!("{khz} kHz is outside the FM band")));
            return;
        }
        audio.close(&self.gst);
        let _ = self.cmds.send(Cmd::PlayFm { khz });
        self.keep_last_fm(khz);
        self.hub.update(|s| {
            quiet(&mut s.dab, &mut s.fm);
            s.fm.active = true;
            s.fm.station = Some(khz);
            s.fm.error = None;
        });
    }

    fn seek(&mut self, audio: &mut Audio, s: &State, up: bool) {
        let from =
            s.fm.station
                .or(s.fm.last_station)
                .unwrap_or(livi_fm::LOWEST_KHZ);
        audio.close(&self.gst);
        let _ = self.cmds.send(Cmd::SeekFm { from, up });
        self.hub.update(|s| {
            quiet(&mut s.dab, &mut s.fm);
            s.fm.active = true;
            s.fm.station = Some(from);
            s.fm.error = None;
        });
    }

    fn keep_last_fm(&mut self, khz: u32) {
        if self.own.last_fm != Some(khz) {
            self.own.last_fm = Some(khz);
            self.keep();
        }
    }

    /// A tuned FM frequency that is not in the list goes in with its star.
    fn favorite(&mut self, s: &State, station: Station, favorite: bool) {
        let changed = match station.band {
            Band::Dab => star(&mut self.own.stations, station.id, favorite),
            Band::Fm => {
                let list = &mut self.own.fm_stations;
                if favorite && !list.iter().any(|st| st.khz == station.id) {
                    let tuned = s.fm.station == Some(station.id);
                    list.push(FmStation {
                        khz: station.id,
                        name: s.fm.name.clone().filter(|_| tuned).unwrap_or_default(),
                        pi: None,
                        favorite: false,
                        hidden: false,
                    });
                }
                star(list, station.id, favorite)
            }
        };
        if changed {
            self.keep();
        }
    }

    /// The name a scan heard first can be a scrolled title; the one RDS settles on replaces it.
    /// Holds no borrow of the state: `keep` publishes into it.
    fn rename_fm(&mut self, tuned: Option<u32>, name: &str) {
        let name = name.trim();
        let Some(khz) = tuned else {
            return;
        };
        let listed = self.own.fm_stations.iter_mut().find(|st| st.khz == khz);
        if let Some(st) = listed.filter(|st| !name.is_empty() && st.name != name) {
            st.name = name.to_string();
            self.keep();
        }
    }

    fn hide(&mut self, station: Station, hidden: bool) {
        let changed = match station.band {
            Band::Dab => list::hide(&mut self.own.stations, station.id, hidden),
            Band::Fm => list::hide(&mut self.own.fm_stations, station.id, hidden),
        };
        if changed {
            self.keep();
        }
    }

    fn move_station(&mut self, station: Station, by: i32) {
        let changed = match station.band {
            Band::Dab => move_station(&mut self.own.stations, station.id, by),
            Band::Fm => move_station(&mut self.own.fm_stations, station.id, by),
        };
        if changed {
            self.keep();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dab_station(id: u32) -> DabStation {
        DabStation {
            service_id: id,
            label: "a".into(),
            short_label: String::new(),
            ensemble: String::new(),
            channel: "5C".into(),
            frequency_hz: 1,
            favorite: false,
            hidden: false,
            logo: None,
        }
    }

    fn fm_station(khz: u32) -> FmStation {
        FmStation {
            khz,
            name: String::new(),
            pi: None,
            favorite: false,
            hidden: false,
        }
    }

    #[test]
    fn manual_gain_snaps_to_a_tuner_step() {
        assert_eq!(nearest_gain(200), 197);
        assert_eq!(nearest_gain(-5), 0);
        assert_eq!(nearest_gain(900), 496);
    }

    #[test]
    fn media_keys_steer_the_radio_while_it_is_the_source() {
        let stick = Dab {
            receiver: Some("stick".into()),
            ..Default::default()
        };
        let (mut dab, mut fm) = (stick.clone(), Fm::default());
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::Next),
            None,
            "no stations, the phone keeps the keys"
        );
        fm.stations = vec![fm_station(98_100)];
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::Next),
            Some(RadioAsk::Step(1)),
            "nothing else plays"
        );
        dab.active = true;
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::Prev),
            Some(RadioAsk::Step(-1))
        );
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::Pause),
            Some(RadioAsk::Pause)
        );
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::PlayPause),
            Some(RadioAsk::Resume(None))
        );
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::Play),
            Some(RadioAsk::Resume(None))
        );
        dab.playing = true;
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::PlayPause),
            Some(RadioAsk::Pause)
        );
        dab = stick;
        fm.active = true;
        fm.playing = true;
        assert_eq!(
            media_ask(&dab, &fm, None, MediaControl::PlayPause),
            Some(RadioAsk::Pause),
            "FM is the source too"
        );
    }

    #[test]
    fn without_a_stick_the_keys_stay_with_the_phone() {
        let dab = Dab {
            active: true,
            playing: true,
            stations: vec![dab_station(1)],
            ..Default::default()
        };
        let fm = Fm {
            active: true,
            stations: vec![fm_station(98_100)],
            ..Default::default()
        };
        for phone in [None, Some(Protocol::Androidauto)] {
            assert_eq!(media_ask(&dab, &fm, phone, MediaControl::Next), None);
        }
    }

    #[test]
    fn carplay_keeps_the_keys_and_resumes_the_last_source_itself() {
        let dab = Dab {
            active: true,
            playing: true,
            stations: vec![dab_station(1)],
            ..Default::default()
        };
        for control in [
            MediaControl::PlayPause,
            MediaControl::Next,
            MediaControl::Prev,
        ] {
            assert_eq!(
                media_ask(&dab, &Fm::default(), Some(Protocol::Carplay), control),
                None
            );
        }
    }

    #[test]
    fn android_auto_hands_the_keys_to_the_radio_only_while_it_plays() {
        let aa = Some(Protocol::Androidauto);
        let mut dab = Dab {
            receiver: Some("stick".into()),
            active: true,
            stations: vec![dab_station(1)],
            ..Default::default()
        };
        assert_eq!(
            media_ask(&dab, &Fm::default(), aa, MediaControl::PlayPause),
            None,
            "paused for the phone, the phone keeps the keys"
        );
        dab.playing = true;
        assert_eq!(
            media_ask(&dab, &Fm::default(), aa, MediaControl::Next),
            Some(RadioAsk::Step(1))
        );
        assert_eq!(
            media_ask(&dab, &Fm::default(), aa, MediaControl::PlayPause),
            Some(RadioAsk::Pause)
        );
    }

    #[test]
    fn a_pulled_stick_hands_the_player_back_to_the_phone() {
        let mut dab = Dab {
            receiver: Some("stick".into()),
            stations: vec![dab_station(1)],
            last_station: Some(1),
            active: true,
            station: Some(1),
            playing: true,
            text: Some("Lesart".into()),
            gains_db: vec![0.0, 0.9],
            ..Dab::default()
        };
        let mut fm = Fm {
            stations: vec![fm_station(98_100)],
            last_station: Some(98_100),
            name: Some("NDR 2".into()),
            ..Fm::default()
        };
        stick_gone(&mut dab, &mut fm);
        assert!(!dab.active && !dab.playing && dab.receiver.is_none());
        assert_eq!((dab.station, dab.text), (None, None));
        assert_eq!(dab.stations.len(), 1, "the list stays");
        assert_eq!(dab.last_station, Some(1));
        assert_eq!(dab.gains_db, [0.0, 0.9], "the gain setting keeps its steps");
        assert_eq!(fm.stations.len(), 1);
        assert_eq!(fm.last_station, Some(98_100));
        assert_eq!(fm.name, None);
    }

    #[test]
    fn rds_names_only_the_station_on_air() {
        let mut fm = Fm::default();
        rds(&mut fm, Rds::Name("NDR 2".into()));
        assert_eq!(fm.name, None, "nothing tuned");
        fm.active = true;
        rds(&mut fm, Rds::Name("NDR 2   ".into()));
        rds(&mut fm, Rds::Text("  ".into()));
        assert_eq!(fm.name.as_deref(), Some("NDR 2"));
        assert_eq!(fm.text, None, "a blank text says nothing");
    }

    #[test]
    fn one_band_plays_at_a_time() {
        let mut dab = Dab {
            active: true,
            station: Some(1),
            ..Dab::default()
        };
        let mut fm = Fm {
            scan: Some(DabScan::default()),
            ..Fm::default()
        };
        quiet(&mut dab, &mut fm);
        assert!(!dab.active && dab.station.is_none() && fm.scan.is_none());
    }
}
