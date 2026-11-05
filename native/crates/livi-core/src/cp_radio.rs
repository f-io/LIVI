// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;

use base64::Engine;
use livi_core_proto::state::{Dab, DabStation, Fm, FmStation};
use livi_cp::manager::{CpCmd, CpHandle};
use livi_cp::vehicle_state::{Radio, RadioBand, RadioCommand, RadioSource, RadioStation};

use crate::hub::Hub;
use crate::radio::{Band, RadioAsk, Station};

pub fn station_id(service: u32) -> String {
    format!("{service:x}")
}

fn band(b: RadioBand) -> Band {
    match b {
        RadioBand::Dab => Band::Dab,
        RadioBand::Fm => Band::Fm,
    }
}

pub fn ask(command: RadioCommand) -> Option<RadioAsk> {
    Some(match command {
        RadioCommand::Tune(RadioBand::Dab, id) => {
            RadioAsk::Play(Station::dab(u32::from_str_radix(&id, 16).ok()?))
        }
        RadioCommand::Tune(RadioBand::Fm, khz) => RadioAsk::Play(Station::fm(khz.parse().ok()?)),
        RadioCommand::TuneFrequency(khz) => RadioAsk::Play(Station::fm(khz)),
        RadioCommand::Source(b) => RadioAsk::Resume(Some(band(b))),
        RadioCommand::Play => RadioAsk::Resume(None),
        RadioCommand::Pause => RadioAsk::Pause,
        RadioCommand::Next => RadioAsk::Step(1),
        RadioCommand::Previous => RadioAsk::Step(-1),
    })
}

fn logo(st: &DabStation) -> Option<Vec<u8>> {
    st.logo
        .as_deref()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
}

fn mhz(khz: u32) -> String {
    format!("{}.{} MHz", khz / 1000, khz % 1000 / 100)
}

fn named(name: &str) -> Option<String> {
    Some(name.trim())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

fn fm_name(st: &FmStation) -> String {
    named(&st.name).unwrap_or_else(|| mhz(st.khz))
}

struct Playing {
    title: String,
    artist: String,
    album: String,
    artwork: Vec<u8>,
    playing: bool,
}

fn heard(name: String, text: &Option<String>) -> (String, String) {
    match text {
        Some(text) => (text.clone(), name),
        None => (name, String::new()),
    }
}

fn dab_playing(dab: &Dab) -> Playing {
    let current = dab.station.or(dab.last_station);
    let tuned = current.and_then(|id| dab.stations.iter().find(|s| s.service_id == id));
    let label = tuned.map(|s| s.label.clone()).unwrap_or_default();
    let (title, artist) = heard(label, &dab.text);
    Playing {
        title,
        artist,
        album: tuned.map(|s| s.ensemble.clone()).unwrap_or_default(),
        artwork: tuned.and_then(logo).unwrap_or_default(),
        playing: dab.playing,
    }
}

fn fm_playing(fm: &Fm) -> Playing {
    let khz = fm.station.or(fm.last_station);
    let listed = khz.and_then(|k| fm.stations.iter().find(|s| s.khz == k));
    let name = fm
        .name
        .as_deref()
        .and_then(named)
        .or_else(|| listed.map(fm_name))
        .or_else(|| khz.map(mhz))
        .unwrap_or_default();
    let (title, artist) = heard(name, &fm.text);
    Playing {
        title,
        artist,
        album: khz.map(mhz).unwrap_or_default(),
        artwork: Vec::new(),
        playing: fm.playing,
    }
}

fn radio(dab: &Dab, fm: &Fm, before: RadioBand) -> Radio {
    let on = if fm.active {
        RadioBand::Fm
    } else if dab.active {
        RadioBand::Dab
    } else {
        before
    };
    let now = match on {
        RadioBand::Dab => dab_playing(dab),
        RadioBand::Fm => fm_playing(fm),
    };
    let fm_khz = fm.station.or(fm.last_station);
    Radio {
        available: dab.receiver.is_some(),
        dab: RadioSource {
            stations: dab
                .stations
                .iter()
                .filter(|s| !s.hidden)
                .map(|s| RadioStation {
                    id: station_id(s.service_id),
                    name: s.label.clone(),
                    short_name: s.short_label.clone(),
                    ensemble: s.ensemble.clone(),
                    frequency: None,
                    image: logo(s),
                })
                .collect(),
            current: dab.station.or(dab.last_station).map(station_id),
        },
        fm: RadioSource {
            stations: fm
                .stations
                .iter()
                .filter(|s| !s.hidden)
                .map(|s| RadioStation {
                    id: s.khz.to_string(),
                    name: fm_name(s),
                    short_name: fm_name(s),
                    ensemble: String::new(),
                    frequency: Some(s.khz),
                    image: None,
                })
                .collect(),
            current: fm_khz.map(|k| k.to_string()),
        },
        band: on,
        frequency: fm_khz,
        title: now.title,
        artist: now.artist,
        album: now.album,
        artwork: now.artwork,
        playing: now.playing,
    }
}

pub async fn run(hub: Arc<Hub>, cp: CpHandle) {
    let mut state = hub.watch();
    let mut seen: Option<(Dab, Fm)> = None;
    let mut on = RadioBand::Dab;
    loop {
        let now = {
            let s = state.borrow_and_update();
            (s.dab.clone(), s.fm.clone())
        };
        if seen.as_ref() != Some(&now) {
            let r = radio(&now.0, &now.1, on);
            on = r.band;
            cp.send(CpCmd::Radio(Box::new(r)));
            seen = Some(now);
        }
        if state.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(id: u32, label: &str) -> DabStation {
        DabStation {
            service_id: id,
            label: label.into(),
            short_label: label.into(),
            ensemble: "DR Deutschland".into(),
            channel: "5C".into(),
            frequency_hz: 178_352_000,
            favorite: false,
            hidden: false,
            logo: None,
        }
    }

    #[test]
    fn the_radio_app_hears_stations_and_what_plays() {
        let kultur = DabStation {
            logo: Some("AQID".into()),
            ..station(0xd220, "Dlf Kultur")
        };
        let gone = DabStation {
            hidden: true,
            ..station(0xd240, "Doku")
        };
        let dab = Dab {
            receiver: Some("stick".into()),
            stations: vec![station(0xd210, "Dlf"), kultur, gone],
            station: Some(0xd220),
            playing: true,
            text: Some("Lesart".into()),
            ..Default::default()
        };
        let r = radio(&dab, &Fm::default(), RadioBand::Dab);
        assert!(r.available);
        assert_eq!(r.dab.current.as_deref(), Some("d220"));
        assert_eq!(
            (r.title.as_str(), r.artist.as_str()),
            ("Lesart", "Dlf Kultur")
        );
        assert_eq!(r.album, "DR Deutschland");
        assert_eq!(r.artwork, vec![1, 2, 3], "the tuned station's logo");
        assert_eq!(r.dab.stations[1].image.as_deref(), Some(&[1u8, 2, 3][..]));
        assert_eq!(r.dab.stations[0].image, None, "no logo yet, no picture");
        assert_eq!(
            r.dab.stations.len(),
            2,
            "a hidden station stays off the phone"
        );
    }

    #[test]
    fn a_stopped_radio_keeps_its_last_station_paused() {
        let dab = Dab {
            receiver: Some("stick".into()),
            stations: vec![station(0xd210, "Dlf")],
            last_station: Some(0xd210),
            ..Default::default()
        };
        let r = radio(&dab, &Fm::default(), RadioBand::Dab);
        assert_eq!(r.dab.current.as_deref(), Some("d210"));
        assert_eq!(r.title, "Dlf");
        assert_eq!(r.album, "DR Deutschland");
        assert!(!r.playing);
    }

    #[test]
    fn without_text_the_station_is_the_title() {
        let dab = Dab {
            station: Some(1),
            ..Default::default()
        };
        let dab = Dab {
            stations: vec![station(1, "Dlf")],
            ..dab
        };
        let r = radio(&dab, &Fm::default(), RadioBand::Dab);
        assert_eq!((r.title.as_str(), r.artist.as_str()), ("Dlf", ""));
        assert!(!r.available);
        assert!(r.artwork.is_empty());
    }

    #[test]
    fn radio_app_commands_become_radio_asks() {
        assert_eq!(
            ask(RadioCommand::Tune(RadioBand::Dab, "d220".into())),
            Some(RadioAsk::Play(Station::dab(0xd220)))
        );
        assert_eq!(ask(RadioCommand::Tune(RadioBand::Dab, "nope".into())), None);
        assert_eq!(
            ask(RadioCommand::Tune(RadioBand::Fm, "99100".into())),
            Some(RadioAsk::Play(Station::fm(99_100)))
        );
        assert_eq!(
            ask(RadioCommand::TuneFrequency(87_600)),
            Some(RadioAsk::Play(Station::fm(87_600)))
        );
        assert_eq!(
            ask(RadioCommand::Source(RadioBand::Fm)),
            Some(RadioAsk::Resume(Some(Band::Fm)))
        );
        assert_eq!(ask(RadioCommand::Play), Some(RadioAsk::Resume(None)));
        assert_eq!(ask(RadioCommand::Pause), Some(RadioAsk::Pause));
        assert_eq!(ask(RadioCommand::Next), Some(RadioAsk::Step(1)));
        assert_eq!(ask(RadioCommand::Previous), Some(RadioAsk::Step(-1)));
    }

    fn fm(khz: u32, name: &str) -> FmStation {
        FmStation {
            khz,
            name: name.into(),
            pi: None,
            favorite: false,
            hidden: false,
        }
    }

    #[test]
    fn fm_reaches_the_phone_as_its_own_source() {
        let dab = Dab {
            receiver: Some("stick".into()),
            stations: vec![station(0xd210, "Dlf")],
            ..Default::default()
        };
        let fm = Fm {
            stations: vec![
                fm(99_100, "NDR 2"),
                fm(103_200, " "),
                FmStation {
                    hidden: true,
                    ..fm(104_000, "Weg")
                },
            ],
            active: true,
            station: Some(99_100),
            playing: true,
            name: Some("NDR2    ".into()),
            text: Some("Nachrichten".into()),
            ..Default::default()
        };
        let r = radio(&dab, &fm, RadioBand::Dab);
        assert_eq!(r.band, RadioBand::Fm);
        assert_eq!(r.frequency, Some(99_100));
        assert_eq!(r.fm.current.as_deref(), Some("99100"));
        let names: Vec<_> = r.fm.stations.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["NDR 2", "103.2 MHz"]);
        assert_eq!(r.fm.stations[0].frequency, Some(99_100));
        assert_eq!(
            (r.title.as_str(), r.artist.as_str(), r.album.as_str()),
            ("Nachrichten", "NDR2", "99.1 MHz")
        );
        assert!(r.artwork.is_empty());
        assert!(r.playing);
        assert_eq!(r.dab.stations.len(), 1, "DAB stays listed beside FM");
    }

    #[test]
    fn a_paused_radio_stays_on_the_band_heard_last() {
        let fm = Fm {
            last_station: Some(90_300),
            ..Default::default()
        };
        let r = radio(&Dab::default(), &fm, RadioBand::Fm);
        assert_eq!(r.band, RadioBand::Fm);
        assert_eq!(r.title, "90.3 MHz");
        assert!(!r.playing);
        let dab = Dab {
            active: true,
            ..Default::default()
        };
        assert_eq!(radio(&dab, &fm, RadioBand::Fm).band, RadioBand::Dab);
    }
}
