// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Deserialize, Serialize};

use crate::state::Screen;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Input {
    Pointer { screen: Screen, points: Vec<Point> },
    Key { code: String, down: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn pointer_on_the_wire() {
        let input = Input::Pointer {
            screen: Screen::Main,
            points: vec![Point {
                id: 0,
                x: 0.25,
                y: 0.5,
                phase: Phase::Down,
            }],
        };
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            json!({ "kind": "pointer", "screen": "main", "points": [{ "id": 0, "x": 0.25, "y": 0.5, "phase": "down" }] })
        );
    }

    #[test]
    fn key_on_the_wire() {
        let input: Input =
            serde_json::from_value(json!({ "kind": "key", "code": "KeyH", "down": true })).unwrap();
        assert_eq!(
            input,
            Input::Key {
                code: "KeyH".into(),
                down: true
            }
        );
    }
}
