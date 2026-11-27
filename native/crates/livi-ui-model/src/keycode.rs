// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const LETTERS: [&str; 26] = [
    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL",
    "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX",
    "KeyY", "KeyZ",
];
const DIGITS: [&str; 10] = [
    "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8",
    "Digit9",
];
const NUMPAD: [&str; 10] = [
    "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7",
    "Numpad8", "Numpad9",
];
const F_KEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

pub fn code(keycode: i32) -> Option<&'static str> {
    let at = |table: &'static [&'static str], first: i32| {
        table.get(usize::try_from(keycode - first).ok()?).copied()
    };
    Some(match keycode {
        4 => "BrowserBack",
        111 => "Escape",
        7..=16 => at(&DIGITS, 7)?,
        19 => "ArrowUp",
        20 => "ArrowDown",
        21 => "ArrowLeft",
        22 => "ArrowRight",
        23 | 66 => "Enter",
        29..=54 => at(&LETTERS, 29)?,
        55 => "Comma",
        56 => "Period",
        57 => "AltLeft",
        58 => "AltRight",
        59 => "ShiftLeft",
        60 => "ShiftRight",
        61 => "Tab",
        62 => "Space",
        67 => "Backspace",
        68 => "Backquote",
        69 => "Minus",
        70 => "Equal",
        71 => "BracketLeft",
        72 => "BracketRight",
        73 => "Backslash",
        74 => "Semicolon",
        75 => "Quote",
        76 => "Slash",
        79 | 85 => "MediaPlayPause",
        82 => "ContextMenu",
        86 => "MediaStop",
        87 | 166 => "MediaTrackNext",
        88 | 167 => "MediaTrackPrevious",
        92 => "PageUp",
        93 => "PageDown",
        112 => "Delete",
        113 => "ControlLeft",
        114 => "ControlRight",
        115 => "CapsLock",
        116 => "ScrollLock",
        117 => "SuperLeft",
        118 => "SuperRight",
        120 => "PrintScreen",
        121 => "Pause",
        122 => "Home",
        123 => "End",
        124 => "Insert",
        126 => "MediaPlay",
        127 => "MediaPause",
        131..=142 => at(&F_KEYS, 131)?,
        143 => "NumLock",
        144..=153 => at(&NUMPAD, 144)?,
        154 => "NumpadDivide",
        155 => "NumpadMultiply",
        156 => "NumpadSubtract",
        157 => "NumpadAdd",
        158 => "NumpadDecimal",
        159 => "NumpadComma",
        160 => "NumpadEnter",
        161 => "NumpadEqual",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_go_by_their_meaning() {
        assert_eq!(code(54), Some("KeyZ"));
        assert_eq!(code(53), Some("KeyY"));
        assert_eq!(code(29), Some("KeyA"));
        assert_eq!(code(8), Some("Digit1"));
        assert_eq!(code(142), Some("F12"));
        assert_eq!(code(153), Some("Numpad9"));
    }

    #[test]
    fn channel_keys_skip_tracks() {
        assert_eq!(code(166), code(87));
        assert_eq!(code(167), code(88));
    }

    #[test]
    fn the_back_key_is_not_escape() {
        assert_eq!(code(4), Some("BrowserBack"));
        assert_eq!(code(111), Some("Escape"));
    }

    #[test]
    fn every_default_binding_is_a_typed_character_or_has_a_keycode() {
        let bindings = serde_json::to_value(livi_core_proto::config::defaults().bindings).unwrap();
        let known: Vec<&str> = (0..=300).filter_map(code).collect();
        for (name, value) in bindings.as_object().unwrap() {
            let bound = value.as_str().unwrap();
            assert!(
                bound.is_empty() || bound.chars().count() == 1 || known.contains(&bound),
                "{name}: {bound}"
            );
        }
    }

    #[test]
    fn unknown_keycodes_have_no_code() {
        assert_eq!(code(0), None);
        assert_eq!(code(600), None);
    }
}
