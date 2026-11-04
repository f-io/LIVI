// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{CustomPage, Label, Node, OptionSource, OptionValue, choice};

pub fn schema() -> Node {
    Node::route(
        "dab",
        Label::new("settings.dab", "DAB+"),
        vec![
            Node::custom(
                Label::new("settings.dabReceiver", "Receiver"),
                "dabGainAuto",
                CustomPage::DabReceiver,
            ),
            Node::route(
                "dab-stations",
                Label::new("media.stationList", "Station list"),
                vec![Node::custom(
                    Label::new("media.stationList", "Station list"),
                    "dabGainAuto",
                    CustomPage::DabStations,
                )],
            )
            .icon("dabStationList"),
            Node::checkbox(
                Label::new("settings.dabGainAuto", "Automatic gain"),
                "dabGainAuto",
            )
            .icon("dabGainAuto"),
            Node::select(Label::new("settings.dabGain", "Gain"), "dabGainDb", vec![])
                .icon("dabGain")
                .display_value()
                .options_from(OptionSource::DabGains)
                .page(Label::new("settings.dabGain", "Gain")),
            Node::number(
                Label::new("settings.dabPpm", "Frequency correction"),
                "dabPpm",
            )
            .icon("dabPpm")
            .range(Some(-100.0), Some(100.0), Some(1.0))
            .default_number(0.0)
            .display_value()
            .page(Label::new("settings.dabPpm", "Frequency correction")),
        ],
    )
    .icon("dab")
}

pub fn fm_schema() -> Node {
    Node::route(
        "fm",
        Label::new("settings.fm", "FM"),
        vec![
            Node::custom(
                Label::new("settings.dabReceiver", "Receiver"),
                "fmDeemphasisUs",
                CustomPage::FmReceiver,
            ),
            Node::route(
                "fm-stations",
                Label::new("media.stationList", "Station list"),
                vec![Node::custom(
                    Label::new("media.stationList", "Station list"),
                    "fmDeemphasisUs",
                    CustomPage::FmStations,
                )],
            )
            .icon("dabStationList"),
            Node::select(
                Label::new("settings.fmDeemphasis", "De-emphasis"),
                "fmDeemphasisUs",
                vec![
                    choice(
                        Label::new("settings.fmDeemphasis50", "50 µs (Europe)"),
                        OptionValue::Number(50.0),
                    ),
                    choice(
                        Label::new("settings.fmDeemphasis75", "75 µs (Americas, Korea)"),
                        OptionValue::Number(75.0),
                    ),
                ],
            )
            .icon("fmDeemphasis")
            .display_value()
            .page(Label::new("settings.fmDeemphasis", "De-emphasis")),
        ],
    )
    .icon("fm")
}
