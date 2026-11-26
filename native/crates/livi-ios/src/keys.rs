// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSSet;
use objc2_ui_kit::{UIPress, UIPressesEvent, UIView, UIWindow};

define_class!(
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "LiviKeyCatcher"]
    struct KeyCatcher;

    unsafe impl NSObjectProtocol for KeyCatcher {}

    impl KeyCatcher {
        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(pressesBegan:withEvent:))]
        fn presses_began(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            forward(presses, true, self.mtm());
        }

        #[unsafe(method(pressesEnded:withEvent:))]
        fn presses_ended(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            forward(presses, false, self.mtm());
        }

        #[unsafe(method(pressesCancelled:withEvent:))]
        fn presses_cancelled(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            forward(presses, false, self.mtm());
        }
    }
);

pub fn attach(window: &UIWindow, mtm: MainThreadMarker) {
    // SAFETY: init on a freshly allocated KeyCatcher, a UIView subclass.
    let catcher: Retained<KeyCatcher> = unsafe { msg_send![KeyCatcher::alloc(mtm), init] };
    window.addSubview(&catcher);
    catcher.becomeFirstResponder();
}

fn forward(presses: &NSSet<UIPress>, down: bool, mtm: MainThreadMarker) {
    for press in presses.iter() {
        let Some(key) = press.key(mtm) else {
            continue;
        };
        let Some(code) = code(key.keyCode().0) else {
            continue;
        };
        let printed = key.characters().to_string();
        let printed = (!printed.is_empty()).then_some(printed);
        let _ = slint::invoke_from_event_loop(move || {
            livi_ui::key(code, printed.as_deref(), down, false);
        });
    }
}

fn code(usage: isize) -> Option<&'static str> {
    Some(match usage {
        0x04 => "KeyA",
        0x05 => "KeyB",
        0x06 => "KeyC",
        0x07 => "KeyD",
        0x08 => "KeyE",
        0x09 => "KeyF",
        0x0A => "KeyG",
        0x0B => "KeyH",
        0x0C => "KeyI",
        0x0D => "KeyJ",
        0x0E => "KeyK",
        0x0F => "KeyL",
        0x10 => "KeyM",
        0x11 => "KeyN",
        0x12 => "KeyO",
        0x13 => "KeyP",
        0x14 => "KeyQ",
        0x15 => "KeyR",
        0x16 => "KeyS",
        0x17 => "KeyT",
        0x18 => "KeyU",
        0x19 => "KeyV",
        0x1A => "KeyW",
        0x1B => "KeyX",
        0x1C => "KeyY",
        0x1D => "KeyZ",
        0x1E => "Digit1",
        0x1F => "Digit2",
        0x20 => "Digit3",
        0x21 => "Digit4",
        0x22 => "Digit5",
        0x23 => "Digit6",
        0x24 => "Digit7",
        0x25 => "Digit8",
        0x26 => "Digit9",
        0x27 => "Digit0",
        0x28 => "Enter",
        0x29 => "Escape",
        0x2A => "Backspace",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "Minus",
        0x2E => "Equal",
        0x2F => "BracketLeft",
        0x30 => "BracketRight",
        0x31 => "Backslash",
        0x33 => "Semicolon",
        0x34 => "Quote",
        0x35 => "Backquote",
        0x36 => "Comma",
        0x37 => "Period",
        0x38 => "Slash",
        0x39 => "CapsLock",
        0x3A => "F1",
        0x3B => "F2",
        0x3C => "F3",
        0x3D => "F4",
        0x3E => "F5",
        0x3F => "F6",
        0x40 => "F7",
        0x41 => "F8",
        0x42 => "F9",
        0x43 => "F10",
        0x44 => "F11",
        0x45 => "F12",
        0x4F => "ArrowRight",
        0x50 => "ArrowLeft",
        0x51 => "ArrowDown",
        0x52 => "ArrowUp",
        0x58 => "NumpadEnter",
        0xE0 => "ControlLeft",
        0xE1 => "ShiftLeft",
        0xE2 => "AltLeft",
        0xE3 => "MetaLeft",
        0xE4 => "ControlRight",
        0xE5 => "ShiftRight",
        0xE6 => "AltRight",
        0xE7 => "MetaRight",
        _ => return None,
    })
}
