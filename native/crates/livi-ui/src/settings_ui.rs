// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use livi_core_client::Client;
use livi_core_proto::message::{Action, MfiFile, Radio as CoreRadio, ToCore};
use livi_ui_model::i18n::Strings;
use livi_ui_model::settings::calibration::{Pattern, set as calibrate, value_at};
use livi_ui_model::settings::color::{Hsl, from_hsl, hex, rgb, to_hsl};
use livi_ui_model::settings::custom::files::Wanted;
use livi_ui_model::settings::custom::icon::icons as icon_patch;
use livi_ui_model::settings::custom::{Watch, set_binding};
use livi_ui_model::settings::node::{CustomPage, Radio};
use livi_ui_model::settings::restart::{pending, sessions_open};
use livi_ui_model::settings::schema;
use livi_ui_model::settings::view::{
    Change, Dialog, Facts, Local, Presence, Row, RowKind as ModelRowKind, Screen, Settings,
    number_text, typed_number,
};
use serde_json::Value;
use slint::{Color, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel, Weak};

use crate::files_ui::{Browser, Picked};
use crate::icon_ui::import;
use crate::icons::icon;
use crate::{
    AppWindow, KnobData, Page, PickData, RowData, RowKind, ScreenKind, SettingsState, ToolData,
};

mod calibrate_touch;
mod capture;
mod dialog;
mod render;

pub use capture::*;
pub use dialog::*;
use render::*;

const SLIDE_EVERY: Duration = Duration::from_millis(100);

struct Controller {
    model: Settings,
    stack: Vec<usize>,
    strings: Strings,
    baseline: Option<Value>,
    hsl: Option<Hsl>,
    last_slide: Option<Instant>,
    rows: Vec<Row>,
    dialog: Option<Dialog>,
    dismissed: Option<String>,
    watching: Vec<Watch>,
    capturing: Option<&'static str>,
    browser: Option<Browser>,
    picking: Option<Wanted>,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) };
    static LOGOS: RefCell<HashMap<String, Option<Image>>> = RefCell::new(HashMap::new());
}

fn with<R>(f: impl FnOnce(&mut Controller) -> R) -> Option<R> {
    CONTROLLER.with_borrow_mut(|c| c.as_mut().map(f))
}

fn refill<T: Clone + PartialEq + 'static>(now: &ModelRc<T>, next: Vec<T>) -> Option<ModelRc<T>> {
    let same = now
        .as_any()
        .downcast_ref::<VecModel<T>>()
        .filter(|m| m.row_count() == next.len());
    let Some(model) = same else {
        return Some(ModelRc::from(Rc::new(VecModel::from(next))));
    };
    for (i, item) in next.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&item) {
            model.set_row_data(i, item);
        }
    }
    None
}

pub fn depth() -> usize {
    with(|c| c.stack.len() + c.browser.as_ref().map_or(0, Browser::depth)).unwrap_or(0)
}

pub fn tr(key: &str, text: &str) -> String {
    with(|c| c.strings.t(key, text)).unwrap_or_else(|| text.to_string())
}

pub fn language() -> String {
    with(|c| c.strings.lang().to_string()).unwrap_or_default()
}

pub fn set_language(lang: &str) {
    let shown = crate::fonts::shown();
    with(|c| {
        if c.strings.lang() != lang || c.strings.scripts() != Some(shown.as_slice()) {
            c.strings = Strings::readable(lang, &shown);
        }
    });
}

pub fn open_path(ui: &AppWindow, client: &Client, path: &str) {
    let keys: Vec<&str> = path.split('/').filter(|k| !k.is_empty()).collect();
    with(|c| {
        if let Some(stack) = c.model.find(&keys) {
            c.stack = stack;
        }
    });
    render(ui, client);
}

pub fn reset_to_root(ui: &AppWindow, client: &Client) {
    with(|c| c.stack.clear());
    render(ui, client);
}

pub fn file_picked(weak: &Weak<AppWindow>, client: &Client, path: &std::path::Path) {
    act(weak, client, |c, _| match c.picking.take() {
        Some(wanted) => (picked(wanted, path), false),
        None => (Vec::new(), false),
    });
}

fn picked(wanted: Wanted, path: &std::path::Path) -> Vec<Change> {
    let mfi = |file: MfiFile| {
        vec![Change::Act(Action::ImportMfi {
            file,
            path: path.to_string_lossy().into_owned(),
        })]
    };
    match wanted {
        Wanted::Picture => match import(path) {
            Ok(pngs) => vec![Change::Config(icon_patch(pngs))],
            Err(e) => {
                println!("[ui] icon import {} failed: {e}", path.display());
                Vec::new()
            }
        },
        Wanted::Certificate => mfi(MfiFile::Certificate),
        Wanted::Identity => mfi(MfiFile::Identity),
    }
}

fn send(client: &Client, changes: Vec<Change>) {
    for change in changes {
        match change {
            Change::Config(patch) => client.act(Action::SetConfig { patch }),
            Change::DongleOn(radio) => client.act(Action::SetDongleRadio {
                radio: match radio {
                    Radio::Wifi => CoreRadio::Wifi,
                    Radio::Bt => CoreRadio::Bt,
                },
                on: true,
            }),
            Change::Act(action) => client.act(action),
        }
    }
}

fn act(
    weak: &Weak<AppWindow>,
    client: &Client,
    f: impl FnOnce(&mut Controller, &Value) -> (Vec<Change>, bool),
) {
    let Some(ui) = weak.upgrade() else { return };
    let state = client.state().clone();
    let Some((changes, _)) = with(|c| {
        let out = f(c, &state);
        if out.1 {
            c.stack.pop();
            c.hsl = None;
        }
        out
    }) else {
        return;
    };
    send(client, changes);
    render(&ui, client);
}

fn throttled(c: &mut Controller) -> bool {
    let now = Instant::now();
    if c.last_slide.is_some_and(|t| now - t < SLIDE_EVERY) {
        return false;
    }
    c.last_slide = Some(now);
    true
}

pub fn wire(ui: &AppWindow, client: &Client, lang: &str) {
    CONTROLLER.with_borrow_mut(|c| {
        *c = Some(Controller {
            model: Settings::new(schema()).with_facts(facts()),
            stack: Vec::new(),
            strings: Strings::readable(lang, &crate::fonts::shown()),
            baseline: None,
            hsl: None,
            last_slide: None,
            rows: Vec::new(),
            dialog: None,
            dismissed: None,
            watching: Vec::new(),
            capturing: None,
            browser: None,
            picking: None,
        });
    });
    let s = ui.global::<SettingsState>();
    let weak = ui.as_weak();
    calibrate_touch::wire(ui, client);

    s.on_back({
        let (weak, client) = (weak.clone(), client.clone());
        move || {
            act(&weak, &client, |c, _| match c.browser.as_mut() {
                Some(b) => {
                    if !b.up() {
                        c.browser = None;
                    }
                    (Vec::new(), false)
                }
                None => (Vec::new(), true),
            })
        }
    });
    s.on_press({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            let row = with(|c| c.rows.get(i as usize).filter(|r| r.enabled).cloned()).flatten();
            let Some(row) = row else { return };
            if let Some(a) = row.action {
                client.act(a);
            }
            if let Some(change) = row.change {
                act(&weak, &client, |_, _| (vec![change], false));
            }
            match row.local {
                Some(Local::Browse(wanted)) => {
                    if crate::system_pick() {
                        with(|c| c.picking = Some(wanted));
                    } else {
                        act(&weak, &client, |c, _| {
                            c.browser = Some(Browser::open(wanted));
                            (Vec::new(), false)
                        });
                    }
                }
                Some(Local::CalibrateTouch(n)) => {
                    if let Some(ui) = weak.upgrade() {
                        calibrate_touch::start(&ui, &client, n);
                    }
                }
                Some(Local::PickEntry(n)) => {
                    act(&weak, &client, |c, _| {
                        let Some(Picked::File(wanted, path)) =
                            c.browser.as_mut().and_then(|b| b.pick(n))
                        else {
                            return (Vec::new(), false);
                        };
                        let changes = picked(wanted, &path);
                        if !changes.is_empty() {
                            c.browser = None;
                        }
                        (changes, false)
                    });
                }
                _ => {}
            }
            if row.local == Some(Local::ToProjection)
                && let Some(ui) = weak.upgrade()
            {
                ui.set_page(Page::Projection);
                ui.invoke_page_chosen(Page::Projection);
            }
        }
    });
    s.on_capture({
        let weak = weak.clone();
        move |i| {
            let Some(ui) = weak.upgrade() else { return };
            let s = ui.global::<SettingsState>();
            with(|c| {
                let Some(row) = c.rows.get(i as usize) else {
                    return;
                };
                let Some(binding) = row.binding else { return };
                c.capturing = Some(binding);
                s.set_capture_title(
                    c.strings
                        .with(
                            "settings.pressKeyFor",
                            "Press a key for “{{name}}”",
                            &[("name", &row.label)],
                        )
                        .into(),
                );
                s.set_capture_hint(
                    c.strings
                        .t("settings.pressEscToCancel", "Press Esc to cancel.")
                        .into(),
                );
                s.set_capture_open(true);
            });
        }
    });
    s.on_capture_cancel({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                stop_capture(&ui);
            }
        }
    });
    s.on_tool({
        let client = client.clone();
        move |i, j| {
            let action = with(|c| {
                c.rows
                    .get(i as usize)
                    .and_then(|r| r.tools.get(j as usize))
                    .filter(|t| t.enabled)
                    .map(|t| t.action.clone())
            })
            .flatten();
            if let Some(a) = action {
                client.act(a);
            }
        }
    });
    s.on_dialog_press({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            act(&weak, &client, |c, _| {
                let Some(row) = c.dialog.as_ref().and_then(|d| d.rows.get(i as usize)) else {
                    return (Vec::new(), false);
                };
                if !row.enabled {
                    return (Vec::new(), false);
                }
                if row.local == Some(Local::CloseDialog) {
                    c.dismissed = c.dialog.as_ref().map(|d| d.key.clone());
                    return (Vec::new(), false);
                }
                (
                    row.action.clone().map(Change::Act).into_iter().collect(),
                    false,
                )
            })
        }
    });
    s.on_apply({
        let (weak, client) = (weak.clone(), client.clone());
        move || {
            client.act(Action::ApplySettings);
            let state = client.state().clone();
            with(|c| c.baseline = Some(state["config"].clone()));
            if let Some(ui) = weak.upgrade() {
                render(&ui, &client);
            }
        }
    });
    s.on_open({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            act(&weak, &client, |c, _| {
                let i = i as usize;
                if c.model.opens(&c.stack, i) {
                    c.stack.push(i);
                    c.hsl = None;
                }
                (Vec::new(), false)
            })
        }
    });
    s.on_toggle({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            act(&weak, &client, |c, state| {
                let Some(row) = c.rows.get(i as usize).cloned() else {
                    return (Vec::new(), false);
                };
                if let Some(change) = row.change {
                    let mut out = vec![change];
                    out.extend(row.action.map(Change::Act));
                    return (out, false);
                }
                (
                    c.model
                        .toggle(&c.stack, row.node, state)
                        .into_iter()
                        .collect(),
                    false,
                )
            })
        }
    });
    s.on_slide({
        let client = client.clone();
        move |i, v| {
            let change = with(|c| {
                throttled(c)
                    .then(|| c.model.level(&c.stack, Some(i as usize), v))
                    .flatten()
            })
            .flatten();
            send(&client, change.into_iter().collect());
        }
    });
    s.on_slide_done({
        let (weak, client) = (weak.clone(), client.clone());
        move |i, v| {
            act(&weak, &client, |c, _| {
                c.last_slide = None;
                (
                    c.model
                        .level(&c.stack, Some(i as usize), v)
                        .into_iter()
                        .collect(),
                    false,
                )
            })
        }
    });
    s.on_level_moved({
        let client = client.clone();
        move |v| {
            let change = with(|c| {
                throttled(c)
                    .then(|| c.model.level(&c.stack, None, v))
                    .flatten()
            })
            .flatten();
            send(&client, change.into_iter().collect());
        }
    });
    s.on_level_done({
        let (weak, client) = (weak.clone(), client.clone());
        move |v| {
            act(&weak, &client, |c, _| {
                c.last_slide = None;
                (
                    c.model.level(&c.stack, None, v).into_iter().collect(),
                    false,
                )
            })
        }
    });
    s.on_cal_moved({
        let client = client.clone();
        move |i, v| {
            let change = with(|c| {
                throttled(c)
                    .then(|| c.model.calibration_key(&c.stack, i as usize))
                    .flatten()
                    .map(|key| calibrate(key, value_at(v)))
            })
            .flatten();
            send(&client, change.into_iter().collect());
        }
    });
    s.on_cal_done({
        let (weak, client) = (weak.clone(), client.clone());
        move |i, v| {
            act(&weak, &client, |c, _| {
                c.last_slide = None;
                let key = c.model.calibration_key(&c.stack, i as usize);
                (
                    key.map(|k| calibrate(k, value_at(v))).into_iter().collect(),
                    false,
                )
            })
        }
    });
    s.on_cal_reset({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            act(&weak, &client, |c, _| {
                let key = c.model.calibration_key(&c.stack, i as usize);
                (key.map(|k| calibrate(k, 1.0)).into_iter().collect(), false)
            })
        }
    });
    s.on_pick({
        let (weak, client) = (weak.clone(), client.clone());
        move |i| {
            act(&weak, &client, |c, state| {
                let changes = c.model.pick(&c.stack, i as usize, state, &c.strings);
                let done = !changes.is_empty();
                (changes, done)
            })
        }
    });
    s.on_number_set({
        let (weak, client) = (weak.clone(), client.clone());
        move |v| {
            act(&weak, &client, |c, state| {
                (
                    c.model
                        .number(&c.stack, f64::from(v), state)
                        .into_iter()
                        .collect(),
                    false,
                )
            })
        }
    });
    s.on_number_commit({
        let (weak, client) = (weak.clone(), client.clone());
        move |text: SharedString| {
            if let Some(v) = typed_number(&text) {
                act(&weak, &client, |c, state| {
                    (
                        c.model.number(&c.stack, v, state).into_iter().collect(),
                        false,
                    )
                });
            }
            if let Some(ui) = weak.upgrade() {
                crate::keys_ui::text_done(&ui);
            }
        }
    });
    s.on_text_commit({
        let (weak, client) = (weak.clone(), client.clone());
        move |text: SharedString| {
            act(&weak, &client, |c, _| {
                let change = c.model.text(&c.stack, &text);
                let done = change.is_some();
                (change.into_iter().collect(), done)
            });
            if let Some(ui) = weak.upgrade() {
                crate::keys_ui::text_done(&ui);
            }
        }
    });
    s.on_color_hex_commit({
        let (weak, client) = (weak.clone(), client.clone());
        move |text: SharedString| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(change) = with(|c| c.model.text(&c.stack, &text)).flatten() else {
                ui.global::<SettingsState>().set_color_hex_invalid(true);
                return;
            };
            ui.global::<SettingsState>().set_color_hex_invalid(false);
            act(&weak, &client, |c, _| {
                c.hsl = None;
                (vec![change], false)
            });
            crate::keys_ui::text_done(&ui);
        }
    });
    s.on_color_slide({
        let weak = weak.clone();
        move |which, v| {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<SettingsState>();
            let custom = state.get_color_custom();
            with(|c| {
                let mut hsl = c.hsl.unwrap_or(Hsl {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                });
                match which {
                    0 => hsl.h = f64::from(v).round(),
                    1 => hsl.s = f64::from(v).round(),
                    _ => hsl.l = f64::from(v).round(),
                }
                c.hsl = Some(hsl);
                show_color(&state, hsl, from_hsl(hsl), custom);
            });
        }
    });
    s.on_color_slide_done({
        let (weak, client) = (weak.clone(), client.clone());
        move |which, v| {
            act(&weak, &client, |c, _| {
                let mut hsl = c.hsl.unwrap_or(Hsl {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                });
                match which {
                    0 => hsl.h = f64::from(v).round(),
                    1 => hsl.s = f64::from(v).round(),
                    _ => hsl.l = f64::from(v).round(),
                }
                c.hsl = Some(hsl);
                (
                    c.model
                        .text(&c.stack, &hex(from_hsl(hsl)))
                        .into_iter()
                        .collect(),
                    false,
                )
            })
        }
    });
    s.on_color_reset({
        let (weak, client) = (weak.clone(), client.clone());
        move || {
            act(&weak, &client, |c, _| {
                c.hsl = None;
                (c.model.color_reset(&c.stack).into_iter().collect(), false)
            })
        }
    });
}

fn facts() -> Facts {
    let years = match option_env!("LIVI_BUILD_DATE").and_then(|d| d.get(..4)) {
        Some(y) if y > "2025" => format!("2025–{y}"),
        _ => "2025".into(),
    };
    Facts {
        name: env!("CARGO_PKG_DESCRIPTION").into(),
        homepage: env!("CARGO_PKG_HOMEPAGE").into(),
        source: "https://github.com/f-io/LIVI".into(),
        copyright: format!("© {years} Lasse Heitgres"),
        license: env!("CARGO_PKG_LICENSE").into(),
        version: env!("CARGO_PKG_VERSION").into(),
        sha: option_env!("LIVI_BUILD_SHA").unwrap_or("dev").into(),
        run: option_env!("LIVI_BUILD_RUN").unwrap_or_default().into(),
    }
}

fn set_watch(client: &Client, w: &Watch, on: bool) {
    match w {
        Watch::LinkSpeed => client.send(&ToCore::LinkSpeed { on }),
        Watch::Act(a) if on => client.act(a.clone()),
        Watch::Act(_) => {}
    }
}

fn follow_watches(client: &Client, now: Vec<Watch>) {
    let before = with(|c| std::mem::replace(&mut c.watching, now.clone())).unwrap_or_default();
    for w in before.iter().filter(|w| !now.contains(w)) {
        set_watch(client, w, false);
    }
    for w in now.iter().filter(|w| !before.contains(w)) {
        set_watch(client, w, true);
    }
}

pub fn render(ui: &AppWindow, client: &Client) {
    let state = client.state().clone();
    with(|c| render_screen(c, &ui.global::<SettingsState>(), &state));
    let now = with(|c| c.model.watches(&c.stack)).unwrap_or_default();
    follow_watches(client, now);
}

pub fn leave(client: &Client) {
    with(|c| c.browser = None);
    follow_watches(client, Vec::new());
}
