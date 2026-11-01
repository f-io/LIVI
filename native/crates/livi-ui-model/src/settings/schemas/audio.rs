// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{Label, Node, OptionSource, OptionValue, Shown, choice, number_choice};

fn volume(key: &'static str, text: &'static str, icon: &'static str, path: &'static str) -> Node {
    Node::slider(Label::new(key, text), path)
        .icon(icon)
        .display_value()
        .shown(Shown::Percent)
        .page(Label::new(key, text))
}

fn system_default() -> crate::settings::node::Choice {
    choice(
        Label::new("settings.audioDeviceSystemDefault", "System default"),
        OptionValue::Text(""),
    )
}

pub fn schema() -> Node {
    Node::route(
        "audio",
        Label::new("settings.audio", "Audio"),
        vec![
            volume("settings.huVolume", "Head Unit", "huVolume", "huVolume"),
            volume("settings.music", "Music", "music", "audioVolume"),
            volume(
                "settings.navigation",
                "Navigation",
                "navigation",
                "navVolume",
            ),
            volume(
                "settings.voiceAssistant",
                "Voice Assistant",
                "voiceAssistant",
                "voiceAssistantVolume",
            ),
            volume(
                "settings.phoneCalls",
                "Phone Calls",
                "phoneCalls",
                "callVolume",
            ),
            volume(
                "settings.systemSounds",
                "System Sounds",
                "systemSounds",
                "systemSoundsVolume",
            ),
            Node::select(
                Label::new("settings.audioOutputDevice", "Audio Output"),
                "audioOutputDevice",
                vec![system_default()],
            )
            .icon("audioOutput")
            .label_path("audioOutputDeviceLabel")
            .display_value()
            .options_from(OptionSource::AudioSinks)
            .page(Label::new("settings.audioOutputDevice", "Audio Output")),
            Node::select(
                Label::new("settings.audioInputDevice", "Audio Input"),
                "audioInputDevice",
                vec![system_default()],
            )
            .icon("audioInput")
            .label_path("audioInputDeviceLabel")
            .display_value()
            .options_from(OptionSource::AudioSources)
            .page(Label::new("settings.audioInputDevice", "Audio Input")),
            Node::select(
                Label::new("settings.samplingFrequency", "Sampling Frequency"),
                "samplingFrequency",
                vec![number_choice("44.1 kHz", 0.0), number_choice("48 kHz", 1.0)],
            )
            .icon("samplingFrequency")
            .display_value()
            .page(Label::new(
                "settings.samplingFrequency",
                "Sampling Frequency",
            )),
            Node::checkbox(
                Label::new(
                    "settings.huVolumeLinkSystem",
                    "Link Head Unit to System Volume",
                ),
                "huVolumeLinkSystem",
            )
            .icon("volumeLink"),
            Node::checkbox(
                Label::new("settings.disableAudio", "Disable Audio"),
                "disableAudioOutput",
            )
            .icon("audioOff"),
        ],
    )
    .icon("audio")
}
