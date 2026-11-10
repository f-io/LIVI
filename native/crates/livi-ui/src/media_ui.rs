// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use livi_core_client::Client;
use livi_core_proto::message::{Action, MediaControl};
use livi_ui_model::media::{Guess, Playback, Pressed, Station};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, Timer, TimerMode, VecModel};

use crate::media::{MediaView, RadioView, Radios};
use crate::{AppWindow, DabState, MediaSource, MediaText, Page, Press, StationData};

const UI_INTERVAL: Duration = Duration::from_millis(120);
const FLASH: Duration = Duration::from_millis(140);
const PICK_HOLD: Duration = Duration::from_millis(6000);

#[derive(Default)]
struct Live {
    playback: Playback,
    session: Option<String>,
    duration_ms: f64,
    radios: Option<Radios>,
    on_air: Option<MediaSource>,
    radio_playing: bool,
    radio_guess: Guess,
    tuned: Option<(u32, bool)>,
    picked: Option<(u32, Instant)>,
    rows: Option<Rows>,
}

struct Rows {
    fm: bool,
    stations: Vec<Station>,
    model: Rc<VecModel<StationData>>,
}

thread_local! {
    static LIVE: RefCell<Live> = RefCell::new(Live::default());
    static TICK: Timer = Timer::default();
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb_u8(r, g, b)
}

fn tick(ui: &AppWindow) {
    let now = Instant::now();
    let keep = LIVE.with_borrow_mut(|l| {
        let shown = l.playback.tick(now);
        ui.set_progress(shown.progress);
        ui.set_elapsed_ms(shown.elapsed_ms as f32);
        ui.set_playing(shown.playing);
        ui.global::<DabState>()
            .set_playing(l.radio_guess.shown(l.radio_playing, now));
        if l.picked.is_some_and(|(_, until)| now >= until) {
            l.picked = None;
            ui.global::<DabState>().set_picked(-1);
        }
        l.playback.running()
            || l.playback.guessing(now)
            || l.radio_guess.pending(now)
            || l.picked.is_some()
    });
    let wanted =
        keep && (ui.get_page() == Page::Media || ui.get_video_shown() || ui.get_mirrored());
    TICK.with(|t| match (wanted, t.running()) {
        (true, false) => {
            let weak = ui.as_weak();
            t.start(TimerMode::Repeated, UI_INTERVAL, move || {
                if let Some(ui) = weak.upgrade() {
                    tick(&ui);
                }
            });
        }
        (false, true) => t.stop(),
        _ => {}
    });
}

pub fn follow(ui: &AppWindow) {
    tick(ui);
}

pub fn apply_media(ui: &AppWindow, client: Option<&Client>, mut m: MediaView) {
    ui.set_track_title(m.title.into());
    ui.set_artist(m.artist.into());
    ui.set_album(m.album.into());
    ui.set_app(m.app.into());
    ui.set_can_seek(m.can_seek);
    ui.set_total_ms(m.duration_ms as f32);
    match m.artwork.take() {
        Some(Some(buf)) => {
            ui.set_artwork(Image::from_rgba8(buf));
            ui.set_has_artwork(true);
        }
        Some(None) => ui.set_has_artwork(false),
        None => {}
    }
    let track = format!("{}\u{1f}{}", ui.get_track_title(), ui.get_artist());
    let new_session = LIVE.with_borrow_mut(|l| {
        let changed = l.session.as_ref().is_some_and(|s| *s != m.session);
        l.session = Some(m.session);
        l.duration_ms = m.duration_ms;
        if changed {
            l.playback.reset();
        }
        l.playback.update(
            &track,
            m.duration_ms,
            m.elapsed_ms,
            m.playing,
            Instant::now(),
        );
        changed
    });
    if new_session && ui.get_show_spectrum() {
        ui.set_show_spectrum(false);
        if let Some(c) = client {
            c.spectrum(false);
        }
    }
    tick(ui);
}

pub fn apply_radio(ui: &AppWindow, r: Radios) {
    let before = path(ui);
    let on_air = if r.dab.radio.active {
        Some(MediaSource::Dab)
    } else if r.fm.radio.active {
        Some(MediaSource::Fm)
    } else {
        None
    };
    ui.global::<DabState>().set_receiver(r.dab.radio.receiver);
    let switched = LIVE.with_borrow_mut(|l| {
        l.radios = Some(r);
        std::mem::replace(&mut l.on_air, on_air) != on_air
    });
    if switched {
        ui.global::<DabState>()
            .set_view(on_air.unwrap_or(MediaSource::Phone));
    }
    render(ui);
    if path(ui) != before {
        crate::report_page(ui);
    }
    tick(ui);
}

fn render(ui: &AppWindow) {
    let s = ui.global::<DabState>();
    let fm = s.get_view() == MediaSource::Fm;
    let Some(radios) = LIVE.with_borrow_mut(|l| l.radios.take()) else {
        return;
    };
    let RadioView {
        radio: d,
        logos,
        tuned_logo,
    } = if fm { &radios.fm } else { &radios.dab };
    let t = ui.global::<MediaText>();
    let source = if fm {
        t.get_fm_source()
    } else {
        t.get_dab_source()
    };
    s.set_tunable(fm);
    s.set_active(d.active);
    s.set_title(d.title.clone().map_or_else(|| source.clone(), Into::into));
    s.set_source(source);
    s.set_text(d.text.as_str().into());
    s.set_ensemble(
        if d.title.is_some() {
            d.ensemble.as_str()
        } else {
            ""
        }
        .into(),
    );
    s.set_stereo(d.stereo);
    s.set_signal_bars(i32::from(d.signal_bars));
    s.set_has_tuned(d.title.is_some());
    s.set_favorite(d.tuned_favorite);
    s.set_has_logo(tuned_logo.is_some());
    if let Some(px) = tuned_logo {
        s.set_logo(Image::from_rgba8(px.clone()));
    }
    s.set_initials(d.tuned_initials.as_str().into());
    s.set_badge(rgb(d.tuned_badge));
    show_stations(&s, fm, &d.stations, logos);
    s.set_tuned_index(d.tuned_index.map_or(-1, |i| i as i32));
    let (playing, tuned) = (
        d.playing,
        d.tuned
            .filter(|_| d.title.is_some())
            .map(|id| (id, d.tuned_favorite)),
    );
    LIVE.with_borrow_mut(|l| {
        l.radios = Some(radios);
        l.radio_playing = playing;
        l.radio_guess.settle(playing);
        l.tuned = tuned;
        if l.picked
            .is_some_and(|(id, _)| playing && l.tuned.is_some_and(|(t, _)| t == id))
        {
            l.picked = None;
            s.set_picked(-1);
        }
    });
}

fn station_row(st: &Station, logo: &Option<crate::media::Pixels>) -> StationData {
    StationData {
        id: st.id as i32,
        number: st.number as i32,
        label: st.label.as_str().into(),
        favorite: st.favorite,
        tuned: st.tuned,
        has_logo: logo.is_some(),
        logo: logo.clone().map(Image::from_rgba8).unwrap_or_default(),
        initials: st.initials.as_str().into(),
        badge: rgb(st.badge),
    }
}

fn show_stations(
    s: &DabState<'_>,
    fm: bool,
    stations: &[Station],
    logos: &[Option<crate::media::Pixels>],
) {
    let marks_only = |a: &Station, b: &Station| {
        (a.id, a.number, &a.label, &a.logo, &a.initials, a.badge)
            == (b.id, b.number, &b.label, &b.logo, &b.initials, b.badge)
    };
    let kept = LIVE.with_borrow_mut(|l| {
        let rows = l.rows.as_mut().filter(|r| r.fm == fm)?;
        if rows.stations == stations {
            return Some(());
        }
        if rows.stations.len() != stations.len()
            || !rows
                .stations
                .iter()
                .zip(stations)
                .all(|(a, b)| marks_only(a, b))
        {
            return None;
        }
        for (i, (old, new)) in rows.stations.iter().zip(stations).enumerate() {
            if old != new
                && let Some(mut row) = rows.model.row_data(i)
            {
                row.favorite = new.favorite;
                row.tuned = new.tuned;
                rows.model.set_row_data(i, row);
            }
        }
        rows.stations = stations.to_vec();
        Some(())
    });
    if kept.is_some() {
        return;
    }
    let model = Rc::new(VecModel::from(
        stations
            .iter()
            .zip(logos)
            .map(|(st, logo)| station_row(st, logo))
            .collect::<Vec<_>>(),
    ));
    s.set_stations(ModelRc::from(model.clone()));
    LIVE.with_borrow_mut(|l| {
        l.rows = Some(Rows {
            fm,
            stations: stations.to_vec(),
            model,
        });
    });
}

/// Shows a source without telling the core, for a page it opened itself.
pub fn show(ui: &AppWindow, view: MediaSource) {
    let d = ui.global::<DabState>();
    if d.get_view() != view {
        d.set_view(view);
        render(ui);
    }
}

pub fn set_view(ui: &AppWindow, view: MediaSource) {
    if ui.global::<DabState>().get_view() == view {
        return;
    }
    show(ui, view);
    crate::report_page(ui);
}

pub fn path(ui: &AppWindow) -> &'static str {
    let d = ui.global::<DabState>();
    match (d.get_receiver(), d.get_view()) {
        (false, _) => "/media",
        (true, MediaSource::Dab) => "/media/dabplus",
        (true, MediaSource::Fm) => "/media/fm",
        (true, MediaSource::Phone) => "/media/phone",
    }
}

pub fn set_texts(ui: &AppWindow, tr: impl Fn(&str, &str) -> String) {
    let t = ui.global::<MediaText>();
    t.set_phone_source(tr("media.phoneSource", "Phone").into());
    t.set_dab_source(tr("media.dabSource", "DAB+").into());
    t.set_fm_source(tr("media.fmSource", "FM").into());
    t.set_no_stations(tr("media.noStations", "No stations yet").into());
    t.set_no_fm_stations(tr("media.noFmStations", "No stations yet").into());
    t.set_no_artwork(tr("media.noArtwork", "No Artwork").into());
}

fn flash(ui: &AppWindow, which: Pressed) {
    let p = ui.global::<Press>();
    let set = move |p: &Press, on: bool| match which {
        Pressed::Play => p.set_play(on),
        Pressed::Next => p.set_next(on),
        Pressed::Prev => p.set_prev(on),
    };
    set(&p, true);
    let weak = ui.as_weak();
    Timer::single_shot(FLASH, move || {
        if let Some(ui) = weak.upgrade() {
            set(&ui.global::<Press>(), false);
        }
    });
}

pub fn pressed_feedback(ui: &AppWindow, which: Pressed) {
    flash(ui, which);
    let now = Instant::now();
    LIVE.with_borrow_mut(|l| match which {
        Pressed::Play if l.on_air.is_some() => l.radio_guess.toggle(l.radio_playing, now),
        Pressed::Play => l.playback.toggled(now),
        Pressed::Prev => l.playback.went_back(),
        Pressed::Next => {}
    });
    tick(ui);
}

pub fn wire(ui: &AppWindow, client: &Client) {
    let weak = ui.as_weak();
    let button = |which: Pressed, control: MediaControl| {
        let c = client.clone();
        let weak = weak.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            pressed_feedback(&ui, which);
            let d = ui.global::<DabState>();
            let view = d.get_view();
            if !d.get_receiver() || view == MediaSource::Phone {
                c.act(Action::PhoneMedia { control });
                return;
            }
            let idle = LIVE.with_borrow(|l| l.on_air != Some(view));
            match view {
                MediaSource::Dab if idle => c.act(Action::DabResume),
                MediaSource::Fm if idle => c.act(Action::FmResume),
                _ => c.act(Action::Media { control }),
            }
        }
    };
    ui.on_play_pause(button(Pressed::Play, MediaControl::PlayPause));
    ui.on_prev(button(Pressed::Prev, MediaControl::Prev));
    ui.on_next(button(Pressed::Next, MediaControl::Next));
    ui.on_seek({
        let (c, weak) = (client.clone(), weak.clone());
        move |f| {
            let position = LIVE.with_borrow_mut(|l| {
                let position = l.duration_ms * f64::from(f);
                l.playback.seek(position, Instant::now());
                position
            });
            if let Some(ui) = weak.upgrade() {
                tick(&ui);
            }
            c.act(Action::Seek {
                position_ms: position.round() as u32,
            });
        }
    });
    let d = ui.global::<DabState>();
    d.on_phone({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                set_view(&ui, MediaSource::Phone);
            }
            c.act(Action::PhoneResume)
        }
    });
    d.on_resume({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                set_view(&ui, MediaSource::Dab);
            }
            c.act(Action::DabResume)
        }
    });
    d.on_resume_fm({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                set_view(&ui, MediaSource::Fm);
            }
            c.act(Action::FmResume)
        }
    });
    d.on_pick({
        let (c, weak) = (client.clone(), weak.clone());
        move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let id = id as u32;
            let d = ui.global::<DabState>();
            let (picked, tuned) =
                LIVE.with_borrow(|l| (l.picked.map(|(p, _)| p), l.tuned.map(|(t, _)| t)));
            if picked == Some(id) || (picked.is_none() && tuned == Some(id) && d.get_playing()) {
                LIVE.with_borrow_mut(|l| l.picked = None);
                d.set_picked(-1);
                pressed_feedback(&ui, Pressed::Play);
                return c.act(Action::DabPause);
            }
            LIVE.with_borrow_mut(|l| l.picked = Some((id, Instant::now() + PICK_HOLD)));
            d.set_picked(id as i32);
            tick(&ui);
            c.act(match d.get_view() {
                MediaSource::Fm => Action::FmPlay { khz: id },
                _ => Action::DabPlay { service_id: id },
            })
        }
    });
    d.on_toggle_favorite({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some((id, favorite)) = LIVE.with_borrow(|l| l.tuned) else {
                return;
            };
            let favorite = !favorite;
            c.act(match ui.global::<DabState>().get_view() {
                MediaSource::Fm => Action::FmFavorite { khz: id, favorite },
                _ => Action::DabFavorite {
                    service_id: id,
                    favorite,
                },
            })
        }
    });
}
