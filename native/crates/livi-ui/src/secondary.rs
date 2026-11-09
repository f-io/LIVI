// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;

use livi_core_client::Client;
use livi_core_proto::state::{Front, Screen};
use livi_ui_model::dashboard::screens::{Secondary, name};
#[cfg(not(target_os = "android"))]
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};

use crate::dashboard_ui::{DashView, fill};
use crate::fullscreen::{Tracked, in_compositor};
use crate::{AppWindow, DashState, SecondaryWindow, Theme};

struct Open {
    spec: Secondary,
    window: SecondaryWindow,
    front: Option<Front>,
    fullscreen: Option<Tracked>,
}

thread_local! {
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

fn title(screen: Screen) -> String {
    if in_compositor() {
        format!("livi:{}", name(screen))
    } else {
        match screen {
            Screen::Cluster => "Cluster".into(),
            Screen::Aux => "Auxiliary".into(),
            Screen::Main => "LIVI".into(),
        }
    }
}

fn copy_theme(main: &AppWindow, w: &SecondaryWindow) {
    let from = main.global::<Theme>();
    let to = w.global::<Theme>();
    to.set_dark(from.get_dark());
    to.set_primary_override(from.get_primary_override());
    to.set_highlight_override(from.get_highlight_override());
    to.set_background_override(from.get_background_override());
    to.set_text_override(from.get_text_override());
}

fn open(main: &AppWindow, client: &Client, spec: &Secondary) -> Option<SecondaryWindow> {
    let w = SecondaryWindow::new().ok()?;
    crate::dashboard_ui::wire(&w.global::<DashState>(), client);
    w.set_window_title(title(spec.screen).into());
    w.set_has_dashboard(spec.dashboard);
    w.set_view_area(crate::view_area(spec.view_area));
    copy_theme(main, &w);
    set_window_texts(&w);
    w.global::<DashState>()
        .set_clock(crate::clock::now_text().into());
    let window = w.window();
    match spec.bounds.as_ref().filter(|_| !in_compositor()) {
        Some(b) => {
            window.set_size(LogicalSize::new(b.width as f32, b.height as f32));
            window.set_position(LogicalPosition::new(b.x as f32, b.y as f32));
        }
        None => window.set_size(LogicalSize::new(spec.width as f32, spec.height as f32)),
    }
    if in_compositor() {
        window.set_fullscreen(true);
    }
    let weak = w.as_weak();
    let screen = name(spec.screen);
    let _ = window.set_rendering_notifier(move |state, _| {
        if let (slint::RenderingState::RenderingSetup, Some(w)) = (state, weak.upgrade())
            && !crate::attach_video(w.window(), screen)
        {
            eprintln!("[ui] the {screen} window has no native handle for the video");
        }
    });
    forward_keys(window);
    w.show().ok()?;
    Some(w)
}

#[cfg(not(target_os = "android"))]
fn forward_keys(window: &slint::Window) {
    window.on_winit_window_event(|_, event| {
        let winit::event::WindowEvent::KeyboardInput { event, .. } = event else {
            return EventResult::Propagate;
        };
        let winit::keyboard::PhysicalKey::Code(physical) = event.physical_key else {
            return EventResult::Propagate;
        };
        let down = event.state == winit::event::ElementState::Pressed;
        crate::key(&format!("{physical:?}"), down, event.repeat);
        EventResult::PreventDefault
    });
}

#[cfg(target_os = "android")]
fn forward_keys(_: &slint::Window) {}

pub fn follow(main: &AppWindow, client: &Client, wanted: Vec<Secondary>) {
    OPEN.with_borrow_mut(|open| {
        open.retain(|o| {
            let keep = wanted.iter().any(|s| s.screen == o.spec.screen);
            if !keep {
                crate::detach_video(name(o.spec.screen));
                let _ = o.window.hide();
                client.show(o.spec.screen, Front::Livi);
            }
            keep
        });
        for spec in wanted {
            match open.iter_mut().find(|o| o.spec.screen == spec.screen) {
                Some(o) if o.spec == spec => {}
                Some(o) => {
                    o.window.set_has_dashboard(spec.dashboard);
                    o.window.set_view_area(crate::view_area(spec.view_area));
                    if let Some(t) = o.fullscreen.as_mut() {
                        t.apply(o.window.window(), spec.kiosk);
                    }
                    o.spec = spec;
                }
                None => {
                    if let Some(window) = open_window(main, client, &spec) {
                        let fullscreen =
                            (!in_compositor()).then(|| Tracked::new(window.window(), spec.kiosk));
                        open.push(Open {
                            spec,
                            window,
                            front: None,
                            fullscreen,
                        });
                    }
                }
            }
        }
    });
}

fn open_window(main: &AppWindow, client: &Client, spec: &Secondary) -> Option<SecondaryWindow> {
    let w = open(main, client, spec);
    if w.is_none() {
        eprintln!("[ui] no window for the {} screen", name(spec.screen));
    }
    w
}

pub fn dash(client: &Client, screen: Screen, view: DashView) {
    OPEN.with_borrow_mut(|open| {
        let Some(o) = open.iter_mut().find(|o| o.spec.screen == screen) else {
            return;
        };
        let front = if view.map() {
            Front::Cluster
        } else {
            Front::Livi
        };
        if o.front != Some(front) {
            o.front = Some(front);
            client.show(screen, front);
        }
        fill(&o.window.global::<DashState>(), view);
    });
}

pub fn theme(main: &AppWindow) {
    OPEN.with_borrow(|open| open.iter().for_each(|o| copy_theme(main, &o.window)));
}

fn set_window_texts(w: &SecondaryWindow) {
    let tr = crate::settings_ui::tr;
    crate::dashboard_ui::set_texts(&w.global::<DashState>(), tr);
    w.set_empty_label(tr("secondary.noContent", "No content on this screen").into());
}

pub fn set_texts() {
    OPEN.with_borrow(|open| open.iter().for_each(|o| set_window_texts(&o.window)));
}

pub fn relayout() {
    OPEN.with_borrow(|open| {
        open.iter()
            .for_each(|o| crate::fonts::relayout(o.window.window()))
    });
}

pub fn tick_clock(text: &str) {
    OPEN.with_borrow(|open| {
        open.iter()
            .for_each(|o| o.window.global::<DashState>().set_clock(text.into()))
    });
}

pub fn poll_fullscreen(client: &Client) {
    let changed: Vec<(Screen, bool)> = OPEN.with_borrow_mut(|open| {
        open.iter_mut()
            .filter_map(|o| {
                let on = o.fullscreen.as_mut()?.changed(o.window.window())?;
                o.spec.kiosk = on;
                Some((o.spec.screen, on))
            })
            .collect()
    });
    for (screen, on) in changed {
        crate::fullscreen::save(client, screen, on);
    }
}
