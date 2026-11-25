// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Text,
    Number,
    Hex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Lower,
    Upper,
    Symbols,
    More,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Press {
    Char(String),
    Backspace,
    Shift,
    Symbols,
    More,
    Letters,
    Space,
    Left,
    Right,
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub label: String,
    pub press: Press,
    pub units: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Text(String),
    Backspace,
    Left,
    Right,
    Enter,
}

#[derive(Debug, Clone)]
pub struct Board {
    mode: Mode,
    german: bool,
    layer: Layer,
    pub row: usize,
    pub col: usize,
}

fn chars(s: &str) -> Vec<Key> {
    s.chars()
        .map(|c| Key {
            label: c.to_string(),
            press: Press::Char(c.to_string()),
            units: 1.0,
        })
        .collect()
}

fn key(label: &str, press: Press, units: f32) -> Key {
    Key {
        label: label.to_string(),
        press,
        units,
    }
}

impl Board {
    pub fn new(mode: Mode, lang: &str) -> Self {
        let mut board = Self {
            mode,
            german: lang.starts_with("de"),
            layer: Layer::Lower,
            row: 0,
            col: 0,
        };
        board.focus_done();
        board
    }

    fn focus_done(&mut self) {
        let rows = self.rows();
        self.row = rows.len().saturating_sub(1);
        self.col = rows
            .last()
            .and_then(|r| r.iter().position(|k| k.press == Press::Done))
            .unwrap_or(0);
    }

    pub fn shifted(&self) -> bool {
        self.layer == Layer::Upper
    }

    pub fn rows(&self) -> Vec<Vec<Key>> {
        match self.mode {
            Mode::Number => vec![
                [chars("789"), vec![key("", Press::Backspace, 1.0)]].concat(),
                chars("456-"),
                chars("123."),
                vec![
                    key("", Press::Left, 1.0),
                    key("0", Press::Char("0".into()), 1.0),
                    key("", Press::Right, 1.0),
                    key("", Press::Done, 1.0),
                ],
            ],
            Mode::Hex => vec![
                chars("123AB"),
                chars("456CD"),
                chars("789EF"),
                vec![
                    key("#", Press::Char("#".into()), 1.0),
                    key("0", Press::Char("0".into()), 1.0),
                    key("", Press::Backspace, 1.5),
                    key("", Press::Done, 1.5),
                ],
            ],
            Mode::Text => self.text_rows(),
        }
    }

    fn text_rows(&self) -> Vec<Vec<Key>> {
        let bottom = |toggle: Key| {
            vec![
                toggle,
                key(",", Press::Char(",".into()), 1.0),
                key("", Press::Left, 1.0),
                key("", Press::Space, 4.0),
                key("", Press::Right, 1.0),
                key(".", Press::Char(".".into()), 1.0),
                key("", Press::Done, 1.5),
            ]
        };
        let letters = |rows: [&str; 3]| {
            let case = |s: &str| {
                if self.layer == Layer::Upper {
                    s.to_uppercase().replace("SS", "ß")
                } else {
                    s.to_string()
                }
            };
            vec![
                chars(&case(rows[0])),
                chars(&case(rows[1])),
                [
                    vec![key("", Press::Shift, 1.5)],
                    chars(&case(rows[2])),
                    vec![key("", Press::Backspace, 1.5)],
                ]
                .concat(),
                bottom(key("?123", Press::Symbols, 1.5)),
            ]
        };
        let symbols = |second: &str, third: &str, switch: Key| {
            vec![
                chars("1234567890"),
                chars(second),
                [
                    vec![switch],
                    chars(third),
                    vec![key("", Press::Backspace, 1.5)],
                ]
                .concat(),
                bottom(key("ABC", Press::Letters, 1.5)),
            ]
        };
        match self.layer {
            Layer::Lower | Layer::Upper if self.german => {
                letters(["qwertzuiopü", "asdfghjklöä", "yxcvbnmß"])
            }
            Layer::Lower | Layer::Upper => letters(["qwertyuiop", "asdfghjkl", "zxcvbnm"]),
            Layer::Symbols => symbols("@#$%&-+()/", "*\"':;!?", key("=\\<", Press::More, 1.5)),
            Layer::More => symbols("~`|^_={}[]", "\\<>€£°§", key("?123", Press::Symbols, 1.5)),
        }
    }

    fn centers(row: &[Key], width: f32) -> Vec<f32> {
        let total: f32 = row.iter().map(|k| k.units).sum();
        let mut at = (width - total) / 2.0;
        row.iter()
            .map(|k| {
                let c = at + k.units / 2.0;
                at += k.units;
                c
            })
            .collect()
    }

    pub fn width(rows: &[Vec<Key>]) -> f32 {
        rows.iter()
            .map(|r| r.iter().map(|k| k.units).sum::<f32>())
            .fold(0.0, f32::max)
    }

    pub fn step(&mut self, dx: i32, dy: i32) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let width = Self::width(&rows);
        let row = self.row.min(rows.len() - 1);
        let col = self.col.min(rows[row].len().saturating_sub(1));
        if dy != 0 {
            let x = Self::centers(&rows[row], width)[col];
            let to = (row as i32 + dy).clamp(0, rows.len() as i32 - 1) as usize;
            self.col = Self::centers(&rows[to], width)
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
                .map_or(0, |(i, _)| i);
            self.row = to;
        } else {
            let at = rows[..row].iter().map(Vec::len).sum::<usize>() + col;
            let total: usize = rows.iter().map(Vec::len).sum();
            let mut left = (at as i32 + dx).clamp(0, total as i32 - 1) as usize;
            self.row = 0;
            while left >= rows[self.row].len() {
                left -= rows[self.row].len();
                self.row += 1;
            }
            self.col = left;
        }
    }

    pub fn press_focused(&mut self) -> Option<Typed> {
        self.press(self.row, self.col)
    }

    pub fn press(&mut self, row: usize, col: usize) -> Option<Typed> {
        let press = self.rows().get(row)?.get(col)?.press.clone();
        self.row = row;
        self.col = col;
        let typed = match press {
            Press::Char(c) => Some(Typed::Text(c)),
            Press::Space => Some(Typed::Text(" ".into())),
            Press::Backspace => Some(Typed::Backspace),
            Press::Left => Some(Typed::Left),
            Press::Right => Some(Typed::Right),
            Press::Done => Some(Typed::Enter),
            Press::Shift => {
                self.layer = if self.shifted() {
                    Layer::Lower
                } else {
                    Layer::Upper
                };
                None
            }
            Press::Symbols => return self.switch(Layer::Symbols),
            Press::More => return self.switch(Layer::More),
            Press::Letters => return self.switch(Layer::Lower),
        };
        if matches!(typed, Some(Typed::Text(_))) && self.shifted() {
            self.layer = Layer::Lower;
        }
        typed
    }

    fn switch(&mut self, layer: Layer) -> Option<Typed> {
        self.layer = layer;
        let rows = self.rows();
        self.row = self.row.min(rows.len() - 1);
        self.col = self.col.min(rows[self.row].len() - 1);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(board: &Board, row: usize) -> String {
        board.rows()[row].iter().map(|k| k.label.as_str()).collect()
    }

    #[test]
    fn the_layout_follows_the_language() {
        assert_eq!(labels(&Board::new(Mode::Text, "de"), 0), "qwertzuiopü");
        assert_eq!(labels(&Board::new(Mode::Text, "en"), 0), "qwertyuiop");
        assert_eq!(labels(&Board::new(Mode::Hex, "en"), 2), "789EF");
    }

    #[test]
    fn it_opens_on_done() {
        for mode in [Mode::Text, Mode::Number, Mode::Hex] {
            let mut board = Board::new(mode, "en");
            assert_eq!(board.press_focused(), Some(Typed::Enter));
        }
    }

    #[test]
    fn shift_holds_for_one_letter() {
        let mut board = Board::new(Mode::Text, "de");
        assert_eq!(board.press(2, 0), None);
        assert_eq!(labels(&board, 2), "YXCVBNMß");
        assert_eq!(board.press(0, 0), Some(Typed::Text("Q".into())));
        assert_eq!(board.press(0, 0), Some(Typed::Text("q".into())));
    }

    #[test]
    fn symbols_switch_and_come_back() {
        let mut board = Board::new(Mode::Text, "en");
        board.press(3, 0);
        assert_eq!(labels(&board, 0), "1234567890");
        board.press(2, 0);
        assert_eq!(labels(&board, 1), "~`|^_={}[]");
        board.press(3, 0);
        assert_eq!(labels(&board, 0), "qwertyuiop");
    }

    #[test]
    fn moving_up_lands_on_the_nearest_key() {
        let mut board = Board::new(Mode::Text, "en");
        board.row = 3;
        board.col = 3;
        board.step(0, -1);
        assert_eq!((board.row, board.col), (2, 4));
        assert_eq!(board.press_focused(), Some(Typed::Text("v".into())));
        board.step(0, -3);
        assert_eq!(board.row, 0);
        board.step(-20, 0);
        assert_eq!(board.col, 0);
        board.step(0, -1);
        assert_eq!(board.row, 0);
    }

    #[test]
    fn sideways_steps_wrap_into_the_next_row() {
        let mut board = Board::new(Mode::Text, "en");
        board.row = 0;
        board.col = 9;
        board.step(1, 0);
        assert_eq!((board.row, board.col), (1, 0));
        board.step(-1, 0);
        assert_eq!((board.row, board.col), (0, 9));
        board.step(-20, 0);
        assert_eq!((board.row, board.col), (0, 0));
        board.step(100, 0);
        assert_eq!(board.press_focused(), Some(Typed::Enter));
    }
}
