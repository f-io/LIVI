// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{CustomPage, Label, Node};

fn page(
    route: &'static str,
    key: &'static str,
    text: &'static str,
    icon: &'static str,
    custom: CustomPage,
) -> Node {
    Node::route(
        route,
        Label::new(key, text),
        vec![Node::custom(Label::new(key, text), "carName", custom)],
    )
    .icon(icon)
}

pub fn schema() -> Node {
    Node::route(
        "system",
        Label::new("settings.system", "System"),
        vec![
            page(
                "about",
                "settings.about",
                "About",
                "about",
                CustomPage::About,
            ),
            Node::route(
                "logs",
                Label::new("settings.logs", "Logs"),
                vec![
                    Node::checkbox(Label::new("settings.debugLogging", "Debug"), "debugLogging")
                        .icon("debugLogging"),
                ],
            )
            .icon("logs"),
            page(
                "softwareUpdate",
                "settings.softwareUpdate",
                "Software Update",
                "softwareUpdate",
                CustomPage::SoftwareUpdate,
            ),
            page(
                "restart",
                "settings.restartSystem",
                "Restart System",
                "restart",
                CustomPage::Restart,
            ),
            page(
                "poweroff",
                "settings.powerOff",
                "Power Off",
                "poweroff",
                CustomPage::PowerOff,
            ),
        ],
    )
    .icon("system")
}
