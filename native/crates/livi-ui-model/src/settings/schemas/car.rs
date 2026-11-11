// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{
    CustomPage, Label, Node, OptionValue, Shown, UnitSource, choice, number_choice, text_choice,
};

const CAR_TYPES: [(&str, &str, f64); 13] = [
    ("Gasoline", "settings.carTypeGasoline", 1.0),
    ("Diesel", "settings.carTypeDiesel", 4.0),
    ("Electric", "settings.carTypeElectric", 10.0),
    ("HybridGasoline", "settings.carTypeHybridGasoline", 101.0),
    ("HybridDiesel", "settings.carTypeHybridDiesel", 102.0),
    ("E85", "settings.carTypeE85", 6.0),
    ("Biodiesel", "settings.carTypeBiodiesel", 5.0),
    ("DieselWinter", "settings.carTypeDieselWinter", 3.0),
    ("LPG", "settings.carTypeLPG", 7.0),
    ("CNG", "settings.carTypeCNG", 8.0),
    ("LNG", "settings.carTypeLNG", 9.0),
    ("Hydrogen", "settings.carTypeHydrogen", 11.0),
    ("Other", "settings.carTypeOther", 12.0),
];

const HAND_LHD: f64 = 0.0;
const HAND_RHD: f64 = 1.0;

fn carplay() -> Node {
    Node::route(
        "theme",
        Label::new("settings.carMakerTheme", "Car Maker Theme"),
        vec![
            Node::text(Label::new("settings.carPlayTheme", "Theme"), "carPlayTheme")
                .icon("uiColors")
                .display_value()
                .page(Label::new("settings.carPlayTheme", "Theme")),
            Node::text(
                Label::new("settings.carPlayVehicleModel", "Vehicle Model"),
                "carPlayVehicleModel",
            )
            .icon("car")
            .display_value()
            .page(Label::new("settings.carPlayVehicleModel", "Vehicle Model")),
        ],
    )
    .icon("wirelessCp")
}

pub fn schema() -> Node {
    Node::route(
        "car",
        Label::new("settings.car", "Car"),
        vec![
            Node::select(
                Label::new("settings.carType", "Fuel Type"),
                "carType",
                CAR_TYPES
                    .iter()
                    .map(|&(name, key, value)| {
                        choice(Label::new(key, name), OptionValue::Number(value))
                    })
                    .collect(),
            )
            .icon("carType")
            .display_value()
            .page(Label::new("settings.carType", "Fuel Type")),
            Node::select(
                Label::new("settings.steeringWheelPosition", "Steering Wheel Position"),
                "hand",
                vec![
                    choice(
                        Label::new("settings.lhdr", "Left-Hand Drive"),
                        OptionValue::Number(HAND_LHD),
                    ),
                    choice(
                        Label::new("settings.rhdr", "Right-Hand Drive"),
                        OptionValue::Number(HAND_RHD),
                    ),
                ],
            )
            .icon("steering")
            .display_value()
            .page(Label::new(
                "settings.steeringWheelPosition",
                "Steering Wheel Position",
            )),
            Node::number(Label::new("settings.maxSpeed", "Max Speed"), "maxSpeedKph")
                .icon("maxSpeed")
                .range(Some(60.0), Some(400.0), None)
                .unit(UnitSource::Speed)
                .display_value()
                .page(Label::new("settings.maxSpeed", "Max Speed")),
            Node::select(
                Label::new("settings.speedScaleStep", "Scale Step"),
                "speedScaleStep",
                vec![
                    number_choice("10", 10.0),
                    number_choice("20", 20.0),
                    number_choice("40", 40.0),
                ],
            )
            .icon("speedScaleStep")
            .display_value()
            .page(Label::new("settings.speedScaleStep", "Scale Step")),
            Node::number(Label::new("settings.maxRpm", "Max RPM"), "maxRpm")
                .icon("maxRpm")
                .range(Some(3000.0), Some(20000.0), Some(1000.0))
                .display_value()
                .shown(Shown::Suffix(" rpm"))
                .page(Label::new("settings.maxRpm", "Max RPM")),
            Node::number(Label::new("settings.redlineRpm", "Redline"), "redlineRpm")
                .icon("redlineRpm")
                .range(Some(0.0), Some(20000.0), Some(100.0))
                .display_value()
                .shown(Shown::Suffix(" rpm"))
                .page(Label::new("settings.redlineRpm", "Redline")),
            Node::select(
                Label::new("settings.speedUnit", "Speed Unit"),
                "speedUnit",
                vec![text_choice("km/h", "kmh"), text_choice("mph", "mph")],
            )
            .icon("speedUnit")
            .display_value()
            .page(Label::new("settings.speedUnit", "Speed Unit")),
            Node::select(
                Label::new("settings.temperatureUnit", "Temperature Unit"),
                "temperatureUnit",
                vec![
                    text_choice("°C", "celsius"),
                    text_choice("°F", "fahrenheit"),
                ],
            )
            .icon("temperatureUnit")
            .display_value()
            .page(Label::new("settings.temperatureUnit", "Temperature Unit")),
            Node::select(
                Label::new("settings.carPlayDockPosition", "CarPlay Dock"),
                "carPlayDockPosition",
                vec![
                    choice(
                        Label::new("settings.carPlayDockBottom", "Bottom"),
                        OptionValue::Text("bottom"),
                    ),
                    choice(
                        Label::new("settings.carPlayDockSide", "Side"),
                        OptionValue::Text("side"),
                    ),
                    choice(
                        Label::new("settings.carPlayDockAuto", "Automatic"),
                        OptionValue::Text("auto"),
                    ),
                ],
            )
            .icon("edgeBottom")
            .display_value()
            .page(Label::new("settings.carPlayDockPosition", "CarPlay Dock")),
            Node::checkbox(
                Label::new("settings.carPlayVehicleClimate", "CarPlay Climate Controls"),
                "carPlayVehicleClimate",
            )
            .icon("carPlayVehicleClimate"),
            Node::custom(
                Label::new("settings.carData", "Car Data"),
                "carType",
                CustomPage::CarReadings,
            ),
            carplay(),
        ],
    )
    .icon("car")
}
