// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{CustomPage, Label, Node, OptionValue, Shown, choice};

fn color(key: &'static str, text: &'static str, path: &'static str) -> Node {
    Node::color(Label::new(key, text), path)
        .display_value()
        .page(Label::new(key, text))
}

fn coordinate(key: &'static str, text: &'static str, path: &'static str, limit: f64) -> Node {
    Node::number(Label::new(key, text), path)
        .icon(path)
        .range(Some(-limit), Some(limit), Some(0.01))
        .default_number(0.0)
        .display_value()
        .page(Label::new(key, text))
}

pub fn schema() -> Node {
    Node::route(
        "appearance",
        Label::new("settings.appearance", "Appearance"),
        vec![
            Node::slider(
                Label::new("settings.displayBrightness", "Display Brightness"),
                "displayBrightness",
            )
            .icon("displayBrightness")
            .display_value()
            .shown(Shown::Percent)
            .page(Label::new(
                "settings.displayBrightness",
                "Display Brightness",
            )),
            Node::checkbox(
                Label::new("settings.displayBrightnessAuto", "Auto Brightness"),
                "displayBrightnessAuto",
            )
            .icon("displayBrightnessAuto"),
            Node::checkbox(Label::new("settings.darkMode", "Dark Mode"), "darkMode")
                .icon("darkMode"),
            Node::select(
                Label::new("settings.uiStyle", "UI Style"),
                "uiStyle",
                vec![
                    choice(
                        Label::new("settings.uiStyleComand", "COMAND"),
                        OptionValue::Text("comand"),
                    ),
                    choice(
                        Label::new("settings.uiStyleStandard", "Standard"),
                        OptionValue::Text("standard"),
                    ),
                ],
            )
            .icon("uiStyle")
            .display_value()
            .page(Label::new("settings.uiStyle", "UI Style")),
            Node::checkbox(
                Label::new("settings.overlayMessages", "Overlay Messages"),
                "overlayMessages",
            )
            .icon("overlayMessages"),
            Node::select(
                Label::new("settings.phoneAppearance", "Day / Night Mode"),
                "appearanceMode",
                vec![
                    choice(
                        Label::new("settings.phoneAppearanceSun", "Sunrise / Sunset"),
                        OptionValue::Text("sun"),
                    ),
                    choice(
                        Label::new("settings.phoneAppearanceDay", "Day"),
                        OptionValue::Text("day"),
                    ),
                    choice(
                        Label::new("settings.phoneAppearanceNight", "Night"),
                        OptionValue::Text("night"),
                    ),
                    choice(
                        Label::new("settings.phoneAppearanceAuto", "Auto (sensor)"),
                        OptionValue::Text("auto"),
                    ),
                ],
            )
            .icon("phoneAppearance")
            .display_value()
            .page(Label::new("settings.phoneAppearance", "Day / Night Mode")),
            coordinate(
                "settings.sunLatitude",
                "Latitude (no GPS)",
                "sunLatitude",
                90.0,
            ),
            coordinate(
                "settings.sunLongitude",
                "Longitude (no GPS)",
                "sunLongitude",
                180.0,
            ),
            Node::route(
                "ui-colors",
                Label::plain("UI Colors"),
                vec![
                    color(
                        "settings.primaryColorDark",
                        "Primary Color Dark",
                        "primaryColorDark",
                    ),
                    color(
                        "settings.highlightColorDark",
                        "Highlight Color Dark",
                        "highlightColorDark",
                    ),
                    color(
                        "settings.backgroundColorDark",
                        "Background Color Dark",
                        "backgroundColorDark",
                    ),
                    color("settings.textColorDark", "Text Color Dark", "textColorDark"),
                    color(
                        "settings.primaryColorLight",
                        "Primary Color Light",
                        "primaryColorLight",
                    ),
                    color(
                        "settings.highlightColorLight",
                        "Highlight Color Light",
                        "highlightColorLight",
                    ),
                    color(
                        "settings.backgroundColorLight",
                        "Background Color Light",
                        "backgroundColorLight",
                    ),
                    color(
                        "settings.textColorLight",
                        "Text Color Light",
                        "textColorLight",
                    ),
                ],
            )
            .icon("uiColors"),
            Node::route(
                "display-contrast-gamma",
                Label::plain("Contrast / Gamma"),
                vec![Node::custom(
                    Label::plain("Contrast / Gamma"),
                    "displayGamma",
                    CustomPage::ContrastGammaCalibration,
                )],
            )
            .icon("contrastGamma"),
            Node::route(
                "display-color",
                Label::plain("Color"),
                vec![Node::custom(
                    Label::plain("Color"),
                    "displayColorR",
                    CustomPage::ColorCalibration,
                )],
            )
            .icon("displayColor"),
            Node::route(
                "ui-icon",
                Label::new("settings.uiIcon", "UI Icon"),
                vec![Node::custom(
                    Label::new("settings.uiIcon", "UI Icon"),
                    "carplayIcon180",
                    CustomPage::IconUploader,
                )],
            )
            .icon("uiIcon"),
        ],
    )
    .icon("appearance")
}
