// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(usb_host)]
mod audio;
#[cfg(usb_host)]
mod dab;
#[cfg(usb_host)]
mod fm;
#[cfg(usb_host)]
mod focus;
#[cfg(usb_host)]
mod list;
#[cfg(usb_host)]
mod receiver;
#[cfg(usb_host)]
mod thread;

use livi_core_proto::message::MediaControl;
use livi_core_proto::state::{Dab, Fm, Protocol};
use tokio::sync::mpsc;

#[cfg(usb_host)]
pub use receiver::run;

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
