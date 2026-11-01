// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;

use base64::Engine;
use livi_core_proto::state::{Dab, DabStation};
use livi_cp::manager::{CpCmd, CpHandle};
use livi_cp::vehicle_state::{Radio, RadioCommand, RadioStation};

use crate::dab::DabAsk;
use crate::hub::Hub;

pub fn station_id(service: u32) -> String {
    format!("{service:x}")
}

pub fn ask(command: RadioCommand) -> Option<DabAsk> {
    Some(match command {
        RadioCommand::Tune(id) => DabAsk::Play(u32::from_str_radix(&id, 16).ok()?),
        RadioCommand::Play => DabAsk::Resume,
        RadioCommand::Pause => DabAsk::Pause,
        RadioCommand::Next => DabAsk::Step(1),
        RadioCommand::Previous => DabAsk::Step(-1),
    })
}

fn logo(st: &DabStation) -> Option<Vec<u8>> {
    st.logo
        .as_deref()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
}

fn radio(dab: &Dab) -> Radio {
    let stations = &dab.stations;
    let tuned = dab
        .station
        .and_then(|id| stations.iter().find(|s| s.service_id == id));
    let label = tuned.map(|s| s.label.clone()).unwrap_or_default();
    let artwork = tuned.and_then(logo).unwrap_or_default();
    Radio {
        available: dab.receiver.is_some(),
        stations: stations
            .iter()
            .filter(|s| !s.hidden)
            .map(|s| RadioStation {
                id: station_id(s.service_id),
                name: s.label.clone(),
                short_name: s.short_label.clone(),
                ensemble: s.ensemble.clone(),
                image: logo(s),
            })
            .collect(),
        current: dab.station.map(station_id),
        title: dab.text.clone().unwrap_or_else(|| label.clone()),
        artist: if dab.text.is_some() {
            label
        } else {
            String::new()
        },
        album: tuned.map(|s| s.ensemble.clone()).unwrap_or_default(),
        artwork,
        playing: dab.playing,
    }
}

pub async fn run(hub: Arc<Hub>, cp: CpHandle) {
    let mut state = hub.watch();
    let mut seen: Option<Dab> = None;
    loop {
        let dab = state.borrow_and_update().dab.clone();
        if seen.as_ref() != Some(&dab) {
            cp.send(CpCmd::Radio(Box::new(radio(&dab))));
            seen = Some(dab);
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
        let r = radio(&dab);
        assert!(r.available);
        assert_eq!(r.current.as_deref(), Some("d220"));
        assert_eq!(
            (r.title.as_str(), r.artist.as_str()),
            ("Lesart", "Dlf Kultur")
        );
        assert_eq!(r.album, "DR Deutschland");
        assert_eq!(r.artwork, vec![1, 2, 3], "the tuned station's logo");
        assert_eq!(r.stations[1].image.as_deref(), Some(&[1u8, 2, 3][..]));
        assert_eq!(r.stations[0].image, None, "no logo yet, no picture");
        assert_eq!(r.stations.len(), 2, "a hidden station stays off the phone");
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
        let r = radio(&dab);
        assert_eq!((r.title.as_str(), r.artist.as_str()), ("Dlf", ""));
        assert!(!r.available);
        assert!(r.artwork.is_empty());
    }

    #[test]
    fn radio_app_commands_become_dab_asks() {
        assert_eq!(
            ask(RadioCommand::Tune("d220".into())),
            Some(DabAsk::Play(0xd220))
        );
        assert_eq!(ask(RadioCommand::Tune("nope".into())), None);
        assert_eq!(ask(RadioCommand::Play), Some(DabAsk::Resume));
        assert_eq!(ask(RadioCommand::Pause), Some(DabAsk::Pause));
        assert_eq!(ask(RadioCommand::Next), Some(DabAsk::Step(1)));
        assert_eq!(ask(RadioCommand::Previous), Some(DabAsk::Step(-1)));
    }
}
