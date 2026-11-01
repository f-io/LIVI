// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use serde_json::Value;

const NAVIGATION: &[&str] = &[
    "left",
    "right",
    "up",
    "down",
    "back",
    "selectDown",
    "selectUp",
    "knobLeft",
    "knobRight",
    "knobUp",
    "knobDown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    None,
    Nav,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focused {
    Plain,
    Slider,
    Number,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub zone: Zone,
    pub focused: Focused,
    pub editing: bool,
    pub projection: bool,
    pub streaming: bool,
    pub telemetry: bool,
    pub settings_depth: usize,
    pub repeat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Ignore,
    ToCore(String),
    CycleSession,
    NavStep(i32),
    NavOpen { enter_main: bool },
    EnterMain,
    ToNav,
    MainStep(i32),
    Activate,
    StartEditing,
    StopEditing,
    Adjust(i32),
    SettingsBack,
    Pager(i32),
}

pub fn binding<'a>(config: &'a Value, name: &str) -> &'a str {
    config["bindings"][name].as_str().unwrap_or("")
}

pub fn bound_to(config: &Value, code: &str) -> Option<String> {
    config["bindings"]
        .as_object()?
        .iter()
        .find(|(_, v)| v.as_str() == Some(code))
        .map(|(k, _)| k.clone())
}

fn or<'a>(v: &'a str, fallback: &'a str) -> &'a str {
    if v.is_empty() { fallback } else { v }
}

pub fn released(code: &str, config: &Value) -> Option<String> {
    (bound_to(config, code).as_deref() == Some("voiceAssistant")).then(|| code.to_string())
}

pub fn decide(code: &str, ctx: Context, config: &Value) -> Outcome {
    let b = |name: &str| binding(config, name);
    let is = |name: &str, fallback: &str| code == or(b(name), fallback);
    let left = is("left", "ArrowLeft");
    let right = is("right", "ArrowRight");
    let up = is("up", "ArrowUp");
    let down = is("down", "ArrowDown");
    let back = (!b("back").is_empty() && code == b("back")) || code == "Escape";
    let enter = code == "Enter" || code == "NumpadEnter";
    let select_down = !b("selectDown").is_empty() && code == b("selectDown");
    let text_entry = ctx.focused == Focused::Text && ctx.editing;
    let number_entry = ctx.focused == Focused::Number && ctx.editing && ctx.zone == Zone::Main;
    let in_nav = ctx.zone == Zone::Nav;
    let in_main = ctx.zone == Zone::Main;
    let form = matches!(
        ctx.focused,
        Focused::Slider | Focused::Number | Focused::Text
    ) && in_main;

    if code == or(b("cycleSession"), "KeyS") && !ctx.repeat && !text_entry {
        return Outcome::CycleSession;
    }

    if ctx.telemetry && !in_nav && !form {
        if left {
            return Outcome::Pager(-1);
        }
        if right {
            return Outcome::Pager(1);
        }
    }

    if form && !ctx.editing && code == "Backspace" {
        return Outcome::Pass;
    }

    let mapped = bound_to(config, code);
    let in_projection = ctx.projection && ctx.streaming && !in_nav;
    if let Some(name) = &mapped {
        let navigation = NAVIGATION.contains(&name.as_str());
        if !text_entry && !number_entry && (in_projection || !navigation) {
            if name == "voiceAssistant" && ctx.repeat {
                return Outcome::Ignore;
            }
            return Outcome::ToCore(code.to_string());
        }
    }

    if text_entry {
        if code == "Escape" || enter {
            return Outcome::StopEditing;
        }
        return Outcome::Pass;
    }

    if number_entry {
        if code == "Escape" || enter {
            return Outcome::StopEditing;
        }
        if up || down {
            return Outcome::Adjust(if up { 1 } else { -1 });
        }
        return Outcome::Pass;
    }

    if in_nav {
        if left || up {
            return Outcome::NavStep(-1);
        }
        if right || down {
            return Outcome::NavStep(1);
        }
        if select_down {
            return Outcome::NavOpen {
                enter_main: !ctx.projection,
            };
        }
        if enter {
            return Outcome::NavOpen { enter_main: false };
        }
    }

    if ctx.zone == Zone::None && (left || right || up || down || enter || select_down) {
        return if ctx.projection {
            Outcome::ToNav
        } else {
            Outcome::EnterMain
        };
    }

    if !in_nav && back {
        if ctx.editing {
            return if ctx.telemetry {
                Outcome::ToNav
            } else {
                Outcome::StopEditing
            };
        }
        if ctx.settings_depth > 0 {
            return Outcome::SettingsBack;
        }
        return Outcome::ToNav;
    }

    if in_main && (enter || select_down) {
        return match ctx.focused {
            Focused::Plain => Outcome::Activate,
            Focused::Slider => Outcome::Ignore,
            Focused::Number | Focused::Text => {
                if ctx.editing {
                    Outcome::StopEditing
                } else {
                    Outcome::StartEditing
                }
            }
        };
    }

    if in_main && (left || up || right || down) {
        let delta = if left || up { -1 } else { 1 };
        return match ctx.focused {
            Focused::Slider if left || right => Outcome::Adjust(delta),
            Focused::Number if ctx.editing => {
                if up || down {
                    Outcome::Adjust(-delta)
                } else {
                    Outcome::Ignore
                }
            }
            _ => Outcome::MainStep(delta),
        };
    }

    Outcome::Ignore
}

pub const HELD_GAP: Duration = Duration::from_millis(600);

pub fn held_factor(held: Duration) -> f32 {
    if held >= Duration::from_secs(3) {
        100.0
    } else if held >= Duration::from_secs(1) {
        10.0
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({ "bindings": {
            "up": "ArrowUp", "down": "ArrowDown", "left": "ArrowLeft", "right": "ArrowRight",
            "selectDown": "Enter", "back": "Backspace", "cycleSession": "KeyS",
            "playPause": "KeyP", "voiceAssistant": "KeyV", "home": "KeyH"
        } })
    }

    fn ctx(zone: Zone) -> Context {
        Context {
            zone,
            focused: Focused::Plain,
            editing: false,
            projection: false,
            streaming: false,
            telemetry: false,
            settings_depth: 0,
            repeat: false,
        }
    }

    #[test]
    fn media_bindings_go_to_core_everywhere() {
        assert_eq!(
            decide("KeyP", ctx(Zone::Main), &config()),
            Outcome::ToCore("KeyP".into())
        );
        assert_eq!(
            decide("KeyH", ctx(Zone::Nav), &config()),
            Outcome::ToCore("KeyH".into())
        );
    }

    #[test]
    fn arrows_go_to_the_phone_only_on_the_projection() {
        let mut c = ctx(Zone::Main);
        assert_eq!(decide("ArrowDown", c, &config()), Outcome::MainStep(1));
        c.projection = true;
        c.streaming = true;
        assert_eq!(
            decide("ArrowDown", c, &config()),
            Outcome::ToCore("ArrowDown".into())
        );
    }

    #[test]
    fn nav_moves_and_opens() {
        assert_eq!(
            decide("ArrowRight", ctx(Zone::Nav), &config()),
            Outcome::NavStep(1)
        );
        assert_eq!(
            decide("ArrowUp", ctx(Zone::Nav), &config()),
            Outcome::NavStep(-1)
        );
        assert_eq!(
            decide("Enter", ctx(Zone::Nav), &config()),
            Outcome::NavOpen { enter_main: true }
        );
    }

    #[test]
    fn back_walks_up_then_to_the_nav() {
        let mut c = ctx(Zone::Main);
        c.settings_depth = 2;
        assert_eq!(decide("Escape", c, &config()), Outcome::SettingsBack);
        c.settings_depth = 0;
        assert_eq!(decide("Backspace", c, &config()), Outcome::ToNav);
    }

    #[test]
    fn sliders_take_left_and_right() {
        let mut c = ctx(Zone::Main);
        c.focused = Focused::Slider;
        assert_eq!(decide("ArrowLeft", c, &config()), Outcome::Adjust(-1));
        assert_eq!(decide("ArrowDown", c, &config()), Outcome::MainStep(1));
    }

    #[test]
    fn holding_speeds_up_in_steps() {
        assert_eq!(held_factor(Duration::from_millis(200)), 1.0);
        assert_eq!(held_factor(Duration::from_millis(1500)), 10.0);
        assert_eq!(held_factor(Duration::from_secs(4)), 100.0);
    }

    #[test]
    fn numbers_edit_with_enter() {
        let mut c = ctx(Zone::Main);
        c.focused = Focused::Number;
        assert_eq!(decide("Enter", c, &config()), Outcome::StartEditing);
        c.editing = true;
        assert_eq!(decide("ArrowUp", c, &config()), Outcome::Adjust(1));
        assert_eq!(decide("ArrowDown", c, &config()), Outcome::Adjust(-1));
        assert_eq!(decide("Digit7", c, &config()), Outcome::Pass);
        assert_eq!(decide("Backspace", c, &config()), Outcome::Pass);
        assert_eq!(decide("ArrowLeft", c, &config()), Outcome::Pass);
        assert_eq!(decide("Enter", c, &config()), Outcome::StopEditing);
    }

    #[test]
    fn text_entry_keeps_its_keys() {
        let mut c = ctx(Zone::Main);
        c.focused = Focused::Text;
        c.editing = true;
        assert_eq!(decide("KeyP", c, &config()), Outcome::Pass);
        assert_eq!(decide("KeyS", c, &config()), Outcome::Pass);
        assert_eq!(decide("Backspace", c, &config()), Outcome::Pass);
        assert_eq!(decide("Escape", c, &config()), Outcome::StopEditing);
    }

    #[test]
    fn nothing_focused_enters_the_page() {
        assert_eq!(
            decide("ArrowDown", ctx(Zone::None), &config()),
            Outcome::EnterMain
        );
        assert_eq!(
            decide("KeyS", ctx(Zone::None), &config()),
            Outcome::CycleSession
        );
    }

    #[test]
    fn push_to_talk_releases() {
        assert_eq!(released("KeyV", &config()), Some("KeyV".into()));
        assert_eq!(released("KeyP", &config()), None);
    }
}
