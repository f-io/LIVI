// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use livi_core_client::Client;
use livi_core_proto::message::{Action, MediaControl};
use livi_ui_model::media::{Guess, Playback, Pressed};
use slint::{Color, ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};

use crate::media::{DabView, MediaView};
use crate::{AppWindow, DabState, MediaText, Page, Press, StationData};

const UI_INTERVAL: Duration = Duration::from_millis(120);
const FLASH: Duration = Duration::from_millis(140);

#[derive(Default)]
struct Live {
    playback: Playback,
    session: Option<String>,
    duration_ms: f64,
    dab_active: bool,
    dab_playing: bool,
    dab_guess: Guess,
    tuned: Option<(u32, bool)>,
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
            .set_playing(l.dab_guess.shown(l.dab_playing, now));
        l.playback.running() || l.playback.guessing(now) || l.dab_guess.pending(now)
    });
    let wanted = keep && ui.get_page() == Page::Media;
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

pub fn apply_dab(ui: &AppWindow, v: DabView) {
    let before = path(ui);
    let s = ui.global::<DabState>();
    let d = v.dab;
    s.set_receiver(d.receiver);
    s.set_active(d.active);
    s.set_title(d.title.clone().unwrap_or_else(|| "DAB+".into()).into());
    s.set_text(d.text.into());
    s.set_ensemble(
        if d.title.is_some() {
            d.ensemble
        } else {
            String::new()
        }
        .into(),
    );
    s.set_signal_bars(i32::from(d.signal_bars));
    s.set_has_tuned(d.title.is_some());
    s.set_favorite(d.tuned_favorite);
    s.set_has_logo(v.tuned_logo.is_some());
    if let Some(px) = v.tuned_logo {
        s.set_logo(Image::from_rgba8(px));
    }
    s.set_initials(d.tuned_initials.into());
    s.set_badge(rgb(d.tuned_badge));
    let stations: Vec<StationData> = d
        .stations
        .into_iter()
        .zip(v.logos)
        .map(|(st, logo)| StationData {
            service_id: st.service_id as i32,
            number: st.number as i32,
            label: st.label.into(),
            favorite: st.favorite,
            tuned: st.tuned,
            has_logo: logo.is_some(),
            logo: logo.map(Image::from_rgba8).unwrap_or_default(),
            initials: st.initials.into(),
            badge: rgb(st.badge),
        })
        .collect();
    s.set_stations(ModelRc::from(Rc::new(VecModel::from(stations))));
    s.set_tuned_index(d.tuned_index.map_or(-1, |i| i as i32));
    let switched = LIVE.with_borrow(|l| l.dab_active != d.active);
    LIVE.with_borrow_mut(|l| {
        l.dab_active = d.active;
        l.dab_playing = d.playing;
        l.dab_guess.settle(d.playing);
        l.tuned = d
            .tuned
            .filter(|_| d.title.is_some())
            .map(|id| (id, d.tuned_favorite));
    });
    if switched {
        set_view(ui, d.active);
    }
    if path(ui) != before {
        crate::report_page(ui);
    }
    tick(ui);
}

pub fn set_view(ui: &AppWindow, dab: bool) {
    let d = ui.global::<DabState>();
    if d.get_view_dab() == dab {
        return;
    }
    d.set_view_dab(dab);
    crate::report_page(ui);
}

pub fn path(ui: &AppWindow) -> &'static str {
    let d = ui.global::<DabState>();
    match (d.get_receiver(), d.get_view_dab()) {
        (false, _) => "/media",
        (true, true) => "/media/dabplus",
        (true, false) => "/media/phone",
    }
}

pub fn set_texts(ui: &AppWindow, tr: impl Fn(&str, &str) -> String) {
    let t = ui.global::<MediaText>();
    t.set_phone_source(tr("media.phoneSource", "Phone").into());
    t.set_dab_source(tr("media.dabSource", "DAB+").into());
    t.set_no_stations(tr("media.noStations", "No stations yet").into());
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
        Pressed::Play if l.dab_active => l.dab_guess.toggle(l.dab_playing, now),
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
            let dab_idle =
                d.get_receiver() && d.get_view_dab() && !LIVE.with_borrow(|l| l.dab_active);
            if dab_idle {
                c.act(Action::DabResume);
            } else {
                c.act(Action::Media { control });
            }
        }
    };
    ui.on_play_pause(button(Pressed::Play, MediaControl::PlayPause));
    ui.on_prev(button(Pressed::Prev, MediaControl::Prev));
    ui.on_next(button(Pressed::Next, MediaControl::Next));
    ui.on_seek({
        let c = client.clone();
        move |f| {
            let total = LIVE.with_borrow(|l| l.duration_ms);
            c.act(Action::Seek {
                position_ms: (total * f64::from(f)).round() as u32,
            });
        }
    });
    let d = ui.global::<DabState>();
    d.on_phone({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                set_view(&ui, false);
            }
            c.act(Action::DabStop)
        }
    });
    d.on_resume({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                set_view(&ui, true);
            }
            c.act(Action::DabResume)
        }
    });
    d.on_pick({
        let c = client.clone();
        move |id| {
            c.act(Action::DabPlay {
                service_id: id as u32,
            })
        }
    });
    d.on_toggle_favorite({
        let c = client.clone();
        move || {
            if let Some((service_id, favorite)) = LIVE.with_borrow(|l| l.tuned) {
                c.act(Action::DabFavorite {
                    service_id,
                    favorite: !favorite,
                });
            }
        }
    });
}
