// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn code(scancode: u16) -> Option<&'static str> {
    Some(match scancode {
        1 => "Escape",
        2 => "Digit1",
        3 => "Digit2",
        4 => "Digit3",
        5 => "Digit4",
        6 => "Digit5",
        7 => "Digit6",
        8 => "Digit7",
        9 => "Digit8",
        10 => "Digit9",
        11 => "Digit0",
        12 => "Minus",
        13 => "Equal",
        14 => "Backspace",
        15 => "Tab",
        16 => "KeyQ",
        17 => "KeyW",
        18 => "KeyE",
        19 => "KeyR",
        20 => "KeyT",
        21 => "KeyY",
        22 => "KeyU",
        23 => "KeyI",
        24 => "KeyO",
        25 => "KeyP",
        26 => "BracketLeft",
        27 => "BracketRight",
        28 => "Enter",
        29 => "ControlLeft",
        30 => "KeyA",
        31 => "KeyS",
        32 => "KeyD",
        33 => "KeyF",
        34 => "KeyG",
        35 => "KeyH",
        36 => "KeyJ",
        37 => "KeyK",
        38 => "KeyL",
        39 => "Semicolon",
        40 => "Quote",
        41 => "Backquote",
        42 => "ShiftLeft",
        43 => "Backslash",
        44 => "KeyZ",
        45 => "KeyX",
        46 => "KeyC",
        47 => "KeyV",
        48 => "KeyB",
        49 => "KeyN",
        50 => "KeyM",
        51 => "Comma",
        52 => "Period",
        53 => "Slash",
        54 => "ShiftRight",
        55 => "NumpadMultiply",
        56 => "AltLeft",
        57 => "Space",
        58 => "CapsLock",
        59 => "F1",
        60 => "F2",
        61 => "F3",
        62 => "F4",
        63 => "F5",
        64 => "F6",
        65 => "F7",
        66 => "F8",
        67 => "F9",
        68 => "F10",
        69 => "NumLock",
        70 => "ScrollLock",
        71 => "Numpad7",
        72 => "Numpad8",
        73 => "Numpad9",
        74 => "NumpadSubtract",
        75 => "Numpad4",
        76 => "Numpad5",
        77 => "Numpad6",
        78 => "NumpadAdd",
        79 => "Numpad1",
        80 => "Numpad2",
        81 => "Numpad3",
        82 => "Numpad0",
        83 => "NumpadDecimal",
        85 => "Lang5",
        86 => "IntlBackslash",
        87 => "F11",
        88 => "F12",
        89 => "IntlRo",
        90 => "Lang3",
        91 => "Lang4",
        92 => "Convert",
        93 => "KanaMode",
        94 => "NonConvert",
        96 => "NumpadEnter",
        97 => "ControlRight",
        98 => "NumpadDivide",
        99 => "PrintScreen",
        100 => "AltRight",
        102 => "Home",
        103 => "ArrowUp",
        104 => "PageUp",
        105 => "ArrowLeft",
        106 => "ArrowRight",
        107 => "End",
        108 => "ArrowDown",
        109 => "PageDown",
        110 => "Insert",
        111 => "Delete",
        113 => "AudioVolumeMute",
        114 => "AudioVolumeDown",
        115 => "AudioVolumeUp",
        117 => "NumpadEqual",
        119 => "Pause",
        121 => "NumpadComma",
        122 => "Lang1",
        123 => "Lang2",
        124 => "IntlYen",
        125 => "SuperLeft",
        126 => "SuperRight",
        127 => "ContextMenu",
        163 => "MediaTrackNext",
        164 => "MediaPlayPause",
        165 => "MediaTrackPrevious",
        166 => "MediaStop",
        183 => "F13",
        184 => "F14",
        185 => "F15",
        186 => "F16",
        187 => "F17",
        188 => "F18",
        189 => "F19",
        190 => "F20",
        191 => "F21",
        192 => "F22",
        193 => "F23",
        194 => "F24",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_keep_their_position_whatever_the_layout() {
        assert_eq!(code(21), Some("KeyY"));
        assert_eq!(code(44), Some("KeyZ"));
        assert_eq!(code(16), Some("KeyQ"));
        assert_eq!(code(30), Some("KeyA"));
        assert_eq!(code(2), Some("Digit1"));
    }

    #[test]
    fn every_default_binding_has_a_scancode() {
        let bindings = serde_json::to_value(livi_core_proto::config::defaults().bindings).unwrap();
        let known: Vec<&str> = (0..=u16::MAX).filter_map(code).collect();
        for (name, value) in bindings.as_object().unwrap() {
            let bound = value.as_str().unwrap();
            assert!(
                bound.is_empty() || known.contains(&bound),
                "{name}: {bound}"
            );
        }
    }

    #[test]
    fn unknown_scancodes_have_no_code() {
        assert_eq!(code(0), None);
        assert_eq!(code(600), None);
    }
}
