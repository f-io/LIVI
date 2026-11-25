// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::rc::Rc;

use livi_ui_model::keyboard::{Board, Key as Cap, Mode, Press, Typed};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{AppWindow, KeyData, KeyKind, KeyRow, Keyboard, KeyboardMode};

thread_local! {
    static BOARD: RefCell<Option<Board>> = const { RefCell::new(None) };
    static SHOWN: RefCell<Vec<Vec<Cap>>> = const { RefCell::new(Vec::new()) };
}

pub fn wire(ui: &AppWindow) {
    let k = ui.global::<Keyboard>();
    k.set_available(cfg!(not(target_os = "android")));
    let weak = ui.as_weak();
    k.on_open(move |mode| {
        let mode = match mode {
            KeyboardMode::Number => Mode::Number,
            KeyboardMode::Hex => Mode::Hex,
            KeyboardMode::Text => Mode::Text,
        };
        BOARD.with_borrow_mut(|b| *b = Some(Board::new(mode, &crate::settings_ui::language())));
        if let Some(ui) = weak.upgrade() {
            render(&ui);
        }
    });
    k.on_close(|| {
        BOARD.with_borrow_mut(|b| *b = None);
        SHOWN.with_borrow_mut(Vec::clear);
    });
    let weak = ui.as_weak();
    k.on_press(move |row, col| {
        if let Some(ui) = weak.upgrade() {
            let typed = BOARD.with_borrow_mut(|b| b.as_mut()?.press(row as usize, col as usize));
            done(&ui, typed);
        }
    });
}

pub fn open() -> bool {
    BOARD.with_borrow(Option::is_some)
}

pub fn step(ui: &AppWindow, dx: i32, dy: i32) {
    BOARD.with_borrow_mut(|b| {
        if let Some(b) = b.as_mut() {
            b.step(dx, dy);
        }
    });
    ui.global::<Keyboard>().set_knob(true);
    render(ui);
}

pub fn press(ui: &AppWindow) {
    let typed = BOARD.with_borrow_mut(|b| b.as_mut()?.press_focused());
    ui.global::<Keyboard>().set_knob(true);
    done(ui, typed);
}

fn done(ui: &AppWindow, typed: Option<Typed>) {
    render(ui);
    let text: SharedString = match typed {
        Some(Typed::Text(s)) => s.into(),
        Some(Typed::Backspace) => Key::Backspace.into(),
        Some(Typed::Left) => Key::LeftArrow.into(),
        Some(Typed::Right) => Key::RightArrow.into(),
        Some(Typed::Enter) => Key::Return.into(),
        None => return,
    };
    let window = ui.window();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}

fn kind(press: &Press) -> KeyKind {
    match press {
        Press::Char(_) => KeyKind::Char,
        Press::Shift => KeyKind::Shift,
        Press::Backspace => KeyKind::Backspace,
        Press::Space => KeyKind::Space,
        Press::Left => KeyKind::Left,
        Press::Right => KeyKind::Right,
        Press::Done => KeyKind::Done,
        Press::Symbols | Press::More | Press::Letters => KeyKind::Layer,
    }
}

fn render(ui: &AppWindow) {
    BOARD.with_borrow(|b| {
        let Some(b) = b else { return };
        let rows = b.rows();
        let k = ui.global::<Keyboard>();
        k.set_columns(Board::width(&rows));
        k.set_shifted(b.shifted());
        k.set_focus_row(b.row as i32);
        k.set_focus_col(b.col as i32);
        if SHOWN.with_borrow(|s| *s == rows) {
            return;
        }
        let rows: Vec<KeyRow> = rows
            .iter()
            .map(|r| KeyRow {
                keys: ModelRc::from(Rc::new(VecModel::from(
                    r.iter()
                        .map(|key| KeyData {
                            label: key.label.clone().into(),
                            units: key.units,
                            kind: kind(&key.press),
                        })
                        .collect::<Vec<_>>(),
                ))),
            })
            .collect();
        k.set_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
        SHOWN.with_borrow_mut(|s| *s = b.rows());
    });
}
