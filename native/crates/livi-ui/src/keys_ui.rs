// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::{Cell, RefCell};
use std::time::Instant;

use livi_core_client::Client;
use livi_core_proto::input::Input;
use livi_core_proto::message::Action;
use livi_ui_model::keys::{
    Context, Focused, HELD_GAP, MirrorKey, Outcome, Zone, bound_to, decide, held_factor, released,
};
use livi_ui_model::media::pressed;
#[cfg(not(target_os = "android"))]
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, Model};

use crate::{
    AppWindow, DabState, DashState, Focus, FocusZone, MediaSource, Page, PhoneState, RowKind,
    ScreenKind, SettingsState,
};

const SEEK_STEP_MS: f32 = 15_000.0;
const LEVEL_STEP: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Back,
    Apply,
    Row(usize, RowKind),
    Tool(usize, usize),
    DialogButton(usize),
    Pick(usize),
    NumberMinus,
    Number,
    NumberPlus,
    Text,
    ColorHex,
    ColorReset,
    ColorSlider(i32),
    PageSlider,
    CalSlider(i32),
    Artwork,
    Prev,
    PlayPause,
    Next,
    Progress,
    Source(MediaSource),
    Favorite,
    Station(usize),
}

thread_local! {
    static TRAIL: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    static HELD: Cell<Option<(Instant, Instant, f32)>> = const { Cell::new(None) };
    static KEY_REPEAT: Cell<bool> = const { Cell::new(false) };
}

fn settings_targets(s: &SettingsState) -> Vec<Target> {
    let mut out = Vec::new();
    let dialog = crate::settings_ui::dialog_buttons(s);
    if !dialog.is_empty() {
        return dialog.into_iter().map(Target::DialogButton).collect();
    }
    if s.get_can_back() {
        out.push(Target::Back);
    }
    if s.get_show_apply() {
        out.push(Target::Apply);
    }
    match s.get_kind() {
        ScreenKind::List => {
            let rows = s.get_rows();
            for (i, r) in rows.iter().enumerate() {
                match r.kind {
                    RowKind::Info | RowKind::Progress => {}
                    RowKind::Item => {
                        out.extend((0..r.tools.row_count()).map(|j| Target::Tool(i, j)))
                    }
                    RowKind::Device => {
                        if r.enabled {
                            out.push(Target::Row(i, RowKind::Device));
                        }
                        out.extend((0..r.tools.row_count()).map(|j| Target::Tool(i, j)))
                    }
                    kind => out.push(Target::Row(i, kind)),
                }
            }
        }
        ScreenKind::Select => out.extend((0..s.get_picks().row_count()).map(Target::Pick)),
        ScreenKind::Number => out.extend([Target::NumberMinus, Target::Number, Target::NumberPlus]),
        ScreenKind::Text => out.push(Target::Text),
        ScreenKind::Color => out.extend([
            Target::ColorHex,
            Target::ColorReset,
            Target::ColorSlider(0),
            Target::ColorSlider(1),
            Target::ColorSlider(2),
        ]),
        ScreenKind::Slider => out.push(Target::PageSlider),
        ScreenKind::Calibration => {
            out.extend((0..s.get_cal_knobs().row_count() as i32).map(Target::CalSlider))
        }
        ScreenKind::Pending => {}
    }
    out
}

fn targets(ui: &AppWindow) -> Vec<Target> {
    match ui.get_page() {
        Page::Settings => settings_targets(&ui.global::<SettingsState>()),
        Page::Media => {
            let mut out = Vec::new();
            let d = ui.global::<DabState>();
            if d.get_receiver() {
                out.extend([
                    Target::Source(MediaSource::Phone),
                    Target::Source(MediaSource::Dab),
                    Target::Source(MediaSource::Fm),
                ]);
            }
            if d.get_receiver() && d.get_view() != MediaSource::Phone {
                if d.get_art_shown() {
                    out.extend([Target::Artwork, Target::Favorite]);
                }
                if d.get_band_shown() {
                    out.extend((0..d.get_stations().row_count()).map(Target::Station));
                }
                return out;
            }
            if ui.global::<Focus>().get_media_art() {
                out.push(Target::Artwork);
            }
            out.extend([Target::Prev, Target::PlayPause, Target::Next]);
            if ui.get_can_seek() {
                out.push(Target::Progress);
            }
            out
        }
        _ => Vec::new(),
    }
}

fn kind(t: Target) -> Focused {
    match t {
        Target::Row(_, RowKind::Slider)
        | Target::ColorSlider(_)
        | Target::PageSlider
        | Target::CalSlider(_)
        | Target::Progress => Focused::Slider,
        Target::Number => Focused::Number,
        Target::Text | Target::ColorHex => Focused::Text,
        _ => Focused::Plain,
    }
}

fn first_index(list: &[Target]) -> i32 {
    list.iter()
        .position(|t| !matches!(t, Target::Back | Target::Apply | Target::NumberMinus))
        .or(if list.is_empty() { None } else { Some(0) })
        .map_or(0, |i| i as i32)
}

fn zone(f: &Focus) -> Zone {
    match f.get_zone() {
        FocusZone::None => Zone::None,
        FocusZone::Nav => Zone::Nav,
        FocusZone::Main => Zone::Main,
    }
}

fn enter_main(ui: &AppWindow) {
    let f = ui.global::<Focus>();
    let list = targets(ui);
    if list.is_empty() {
        f.set_zone(FocusZone::None);
        return;
    }
    f.set_editing(false);
    f.set_main_index(first_index(&list));
    f.set_zone(FocusZone::Main);
}

fn to_nav(ui: &AppWindow) {
    let f = ui.global::<Focus>();
    let tabs = ui.get_tabs();
    let at = (0..tabs.row_count())
        .find(|&i| tabs.row_data(i).is_some_and(|t| t.page == ui.get_page()))
        .unwrap_or(0);
    f.set_editing(false);
    f.set_nav_index(at as i32);
    f.set_zone(FocusZone::Nav);
}

fn settle_after_settings_move(ui: &AppWindow, opened: bool, from: i32) {
    let f = ui.global::<Focus>();
    f.set_editing(false);
    let list = targets(ui);
    let index = if opened {
        TRAIL.with_borrow_mut(|t| t.push(from));
        first_index(&list)
    } else {
        TRAIL
            .with_borrow_mut(|t| t.pop())
            .unwrap_or_else(|| first_index(&list))
    };
    f.set_main_index(index.clamp(0, list.len().saturating_sub(1) as i32));
}

fn activate(ui: &AppWindow, target: Target) {
    let s = ui.global::<SettingsState>();
    let d = ui.global::<DabState>();
    let from = ui.global::<Focus>().get_main_index();
    match target {
        Target::Back => {
            s.invoke_back();
            settle_after_settings_move(ui, false, from);
        }
        Target::Apply => s.invoke_apply(),
        Target::Row(i, kind) => {
            let node = s.get_rows().row_data(i).map_or(0, |r| r.node);
            match kind {
                RowKind::Toggle => s.invoke_toggle(i as i32),
                RowKind::Button | RowKind::Device | RowKind::Pick | RowKind::Picture => {
                    let depth = crate::settings_ui::depth();
                    s.invoke_press(i as i32);
                    let now = crate::settings_ui::depth();
                    if now > depth {
                        settle_after_settings_move(ui, true, from);
                    } else if now < depth {
                        TRAIL.with_borrow_mut(|t| {
                            t.truncate(t.len().saturating_sub(depth - now - 1))
                        });
                        settle_after_settings_move(ui, false, from);
                    }
                }
                RowKind::KeyBinding => s.invoke_capture(i as i32),
                RowKind::Slider | RowKind::Devices => {}
                _ => {
                    let depth = crate::settings_ui::depth();
                    s.invoke_open(node);
                    if crate::settings_ui::depth() > depth {
                        settle_after_settings_move(ui, true, from);
                    }
                }
            }
        }
        Target::Tool(i, j) => s.invoke_tool(i as i32, j as i32),
        Target::DialogButton(i) => s.invoke_dialog_press(i as i32),
        Target::Pick(i) => {
            let depth = crate::settings_ui::depth();
            s.invoke_pick(i as i32);
            if crate::settings_ui::depth() < depth {
                settle_after_settings_move(ui, false, from);
            }
        }
        Target::ColorReset => s.invoke_color_reset(),
        Target::NumberMinus => adjust(ui, Target::Number, -1),
        Target::NumberPlus => adjust(ui, Target::Number, 1),
        Target::Artwork => {
            let on = !ui.get_show_spectrum();
            ui.set_show_spectrum(on);
            ui.invoke_spectrum_changed(on);
        }
        Target::Prev => ui.invoke_prev(),
        Target::PlayPause => ui.invoke_play_pause(),
        Target::Next => ui.invoke_next(),
        Target::Source(MediaSource::Phone) => d.invoke_phone(),
        Target::Source(MediaSource::Dab) => d.invoke_resume(),
        Target::Source(MediaSource::Fm) => d.invoke_resume_fm(),
        Target::Favorite => d.invoke_toggle_favorite(),
        Target::Station(i) => {
            if let Some(st) = d.get_stations().row_data(i) {
                d.invoke_pick(st.id);
            }
        }
        _ => {}
    }
}

fn adjust(ui: &AppWindow, target: Target, delta: i32) {
    let s = ui.global::<SettingsState>();
    let d = delta as f32;
    match target {
        Target::Row(i, RowKind::Slider) => {
            if let Some(r) = s.get_rows().row_data(i) {
                s.invoke_slide_done(r.node, (r.level + d * LEVEL_STEP).clamp(0.0, 1.0));
            }
        }
        Target::PageSlider => s.invoke_level_done((s.get_level() + d * LEVEL_STEP).clamp(0.0, 1.0)),
        Target::CalSlider(i) => {
            if let Some(k) = s.get_cal_knobs().row_data(i as usize) {
                s.invoke_cal_done(i, (k.level + d * LEVEL_STEP).clamp(0.0, 1.0));
            }
        }
        Target::ColorSlider(w) => {
            let (now, max) = match w {
                0 => (s.get_color_hue(), 359.0),
                1 => (s.get_color_saturation(), 100.0),
                _ => (s.get_color_lightness(), 100.0),
            };
            s.invoke_color_slide_done(w, (now + d).clamp(0.0, max));
        }
        Target::Number => {
            let now = Instant::now();
            let (started, from) = match HELD.get() {
                Some((started, last, v)) if now - last < HELD_GAP => {
                    (if KEY_REPEAT.get() { started } else { now }, v)
                }
                _ => (now, s.get_number_value()),
            };
            let mut v = from + d * s.get_number_step() * held_factor(now - started);
            if s.get_number_has_min() {
                v = v.max(s.get_number_min());
            }
            if s.get_number_has_max() {
                v = v.min(s.get_number_max());
            }
            HELD.set(Some((started, now, v)));
            s.invoke_number_set(v);
        }
        Target::Progress => {
            let total = ui.get_total_ms();
            if total > 0.0 {
                let at = (ui.get_elapsed_ms() + d * SEEK_STEP_MS).clamp(0.0, total);
                ui.invoke_seek(at / total);
            }
        }
        _ => {}
    }
}

fn open_tab(ui: &AppWindow, enter: bool) {
    let f = ui.global::<Focus>();
    let Some(tab) = ui.get_tabs().row_data(f.get_nav_index().max(0) as usize) else {
        return;
    };
    ui.set_page(tab.page);
    ui.invoke_page_chosen(tab.page);
    TRAIL.with_borrow_mut(Vec::clear);
    if enter && tab.page != Page::Projection {
        enter_main(ui);
    }
}

fn apply(ui: &AppWindow, client: &Client, code: &str, outcome: Outcome) -> bool {
    let f = ui.global::<Focus>();
    let list = targets(ui);
    let current = list.get(f.get_main_index().max(0) as usize).copied();
    match outcome {
        Outcome::Pass => return false,
        Outcome::Ignore => {}
        Outcome::ToCore(code) => {
            let name = bound_to(&client.mirror().json["config"], &code);
            if let Some(p) = name.as_deref().and_then(pressed) {
                crate::media_ui::pressed_feedback(ui, p);
            }
            client.input(Input::Key { code, down: true });
        }
        Outcome::CycleSession => {
            client.act(Action::NextDevice);
            let name = livi_ui_model::overlay::cycled(&client.mirror().state);
            if let Some(name) = name {
                crate::overlay_ui::phone(ui, &name);
            }
        }
        Outcome::NavStep(d) => {
            let n = ui.get_tabs().row_count() as i32;
            if f.get_zone() != FocusZone::Nav {
                to_nav(ui);
            } else if n > 0 {
                f.set_nav_index((f.get_nav_index() + d).clamp(0, n - 1));
            }
        }
        Outcome::NavOpen { enter_main } => open_tab(ui, enter_main),
        Outcome::EnterMain => enter_main(ui),
        Outcome::ToNav => to_nav(ui),
        Outcome::MainStep(d) => {
            if !list.is_empty() {
                f.set_editing(false);
                f.set_main_index((f.get_main_index() + d).clamp(0, list.len() as i32 - 1));
            }
        }
        Outcome::Activate => {
            if let Some(t) = current {
                activate(ui, t);
            }
        }
        Outcome::StartEditing => f.set_editing(true),
        Outcome::StopEditing => {
            let enter = code == "Enter" || code == "NumpadEnter";
            if enter && current.is_some_and(|t| matches!(kind(t), Focused::Text | Focused::Number))
            {
                return false;
            }
            f.set_editing(false);
        }
        Outcome::Adjust(d) => {
            if let Some(t) = current {
                adjust(ui, t, d);
            }
        }
        Outcome::SettingsBack => {
            let from = f.get_main_index();
            ui.global::<SettingsState>().invoke_back();
            settle_after_settings_move(ui, false, from);
        }
        Outcome::Pager(by) => client.act(Action::StepClusterLayout { by }),
    }
    true
}

fn context(ui: &AppWindow, repeat: bool) -> Context {
    let f = ui.global::<Focus>();
    let list = targets(ui);
    let mut z = zone(&f);
    if z == Zone::Main && list.is_empty() {
        z = Zone::None;
    }
    let focused = list
        .get(f.get_main_index().max(0) as usize)
        .copied()
        .map_or(Focused::Plain, kind);
    let page = ui.get_page();
    Context {
        zone: z,
        focused,
        editing: f.get_editing(),
        projection: page == Page::Projection,
        streaming: ui.get_phone() == PhoneState::Streaming,
        telemetry: page == Page::Telemetry,
        settings_depth: if page == Page::Settings {
            crate::settings_ui::depth()
        } else {
            0
        },
        first: if page == Page::Telemetry {
            ui.global::<DashState>().get_page_index() <= 0
        } else {
            f.get_main_index() <= 0
        },
        repeat,
    }
}

pub fn key(ui: &AppWindow, client: &Client, code: &str, down: bool, repeat: bool) -> bool {
    ui.invoke_wake_nav();
    if crate::settings_ui::capturing() {
        if down && !repeat {
            crate::settings_ui::captured(ui, client, code);
        }
        return true;
    }
    let config = client.mirror().json["config"].clone();
    if down && crate::video_ui::mirror_on_screen(ui) {
        match livi_ui_model::keys::mirror_key(code, &config, ui.get_mirror_shown()) {
            MirrorKey::Leave => {
                crate::video_ui::leave_mirror(ui);
                return true;
            }
            MirrorKey::Show => {
                crate::video_ui::wake_mirror(ui);
                return true;
            }
            MirrorKey::Pass => {}
        }
    }
    if !down {
        if let Some(code) = released(code, &config) {
            client.input(Input::Key { code, down: false });
        }
        return false;
    }
    KEY_REPEAT.set(repeat);
    let ctx = context(ui, repeat);
    let outcome = decide(code, ctx, &config);
    apply(ui, client, code, outcome)
}

pub fn touched(ui: &AppWindow) {
    let f = ui.global::<Focus>();
    if !f.get_editing() {
        f.set_zone(FocusZone::None);
    }
    let later = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        if let Some(ui) = later.upgrade() {
            ui.invoke_wake_nav();
        }
    });
}

#[cfg(not(target_os = "android"))]
pub fn wire(ui: &AppWindow, client: &Client) {
    let weak = ui.as_weak();
    let client = client.clone();
    ui.window().on_winit_window_event(move |_, event| {
        let Some(ui) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        match event {
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                let winit::keyboard::PhysicalKey::Code(physical) = event.physical_key else {
                    return EventResult::Propagate;
                };
                let down = event.state == winit::event::ElementState::Pressed;
                if key(&ui, &client, &format!("{physical:?}"), down, event.repeat) {
                    EventResult::PreventDefault
                } else {
                    EventResult::Propagate
                }
            }
            winit::event::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                crate::system_scale_changed(&ui, *scale_factor as f32);
                EventResult::Propagate
            }
            winit::event::WindowEvent::Resized(_) => {
                let later = weak.clone();
                slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                    if let Some(ui) = later.upgrade() {
                        crate::apply_zoom(&ui);
                    }
                });
                EventResult::Propagate
            }
            winit::event::WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                ..
            }
            | winit::event::WindowEvent::Touch(_) => {
                touched(&ui);
                if let winit::event::WindowEvent::Touch(touch) = event
                    && crate::touch_ui::winit(&ui, &client, touch)
                {
                    return EventResult::PreventDefault;
                }
                EventResult::Propagate
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                crate::cursor_ui::moved(&ui, *position);
                if crate::video_ui::mirror_on_screen(&ui) {
                    crate::video_ui::wake_mirror(&ui);
                }
                ui.invoke_wake_nav();
                EventResult::Propagate
            }
            winit::event::WindowEvent::MouseWheel { .. } => {
                ui.invoke_wake_nav();
                EventResult::Propagate
            }
            _ => EventResult::Propagate,
        }
    });
}

pub fn text_done(ui: &AppWindow) {
    ui.global::<Focus>().set_editing(false);
}
