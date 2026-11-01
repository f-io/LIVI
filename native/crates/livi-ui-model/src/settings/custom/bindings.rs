// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::defaults;
use livi_core_proto::message::Action;
use serde_json::Value;

use crate::settings::value::patch;
use crate::settings::view::{Row, RowKind, Tool};

pub fn key_label(code: &str) -> String {
    if let Some(rest) = code
        .strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"))
    {
        return rest.to_string();
    }
    match code {
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "Enter" | "NumpadEnter" => "Enter".into(),
        "Escape" => "Esc".into(),
        "Space" => "Space".into(),
        _ => code.to_string(),
    }
}

pub fn default_binding(binding: &str) -> String {
    serde_json::to_value(defaults().bindings)
        .ok()
        .and_then(|v| v[binding].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn set_binding(binding: &str, code: &str) -> Action {
    Action::SetConfig {
        patch: patch(&format!("bindings.{binding}"), Value::from(code)),
    }
}

pub fn binding_row(
    label: String,
    icon: Option<&'static str>,
    binding: &'static str,
    config: &Value,
) -> Row {
    let current = config["bindings"][binding]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let default = default_binding(binding);
    Row {
        kind: RowKind::KeyBinding,
        label,
        icon,
        value: if current.is_empty() {
            "---".into()
        } else {
            key_label(&current)
        },
        enabled: true,
        binding: Some(binding),
        tools: vec![
            Tool {
                icon: "CloseOutlined",
                label: String::new(),
                on: false,
                tint: None,
                enabled: !current.is_empty(),
                switch: false,
                action: set_binding(binding, ""),
            },
            Tool {
                icon: "RestartAltOutlined",
                label: String::new(),
                on: false,
                tint: None,
                enabled: current != default,
                switch: false,
                action: set_binding(binding, &default),
            },
        ],
        ..Row::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keys_read_like_the_keyboard() {
        assert_eq!(key_label("KeyP"), "P");
        assert_eq!(key_label("Digit7"), "7");
        assert_eq!(key_label("ArrowLeft"), "←");
        assert_eq!(key_label("MediaPlayPause"), "MediaPlayPause");
    }

    #[test]
    fn a_changed_binding_can_be_reset() {
        let row = binding_row(
            "Play/Pause".into(),
            None,
            "playPause",
            &json!({ "bindings": { "playPause": "KeyX" } }),
        );
        assert_eq!(row.value, "X");
        assert!(row.tools[0].enabled && row.tools[1].enabled);
        assert_eq!(row.tools[1].action, set_binding("playPause", "KeyP"));
        let unbound = binding_row("x".into(), None, "playPause", &json!({ "bindings": {} }));
        assert_eq!(unbound.value, "---");
        assert!(!unbound.tools[0].enabled);
    }
}
