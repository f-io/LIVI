// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{
    Choice, Condition, CustomPage, Label, Node, OnPick, OptionSource, OptionValue, Radio, Screen,
    Shown, choice, number_choice,
};

const MIN_HEIGHT: f64 = 200.0;
const MIN_WIDTH: f64 = 300.0;
const MAX_WIDTH: f64 = 4096.0;
const MAX_HEIGHT: f64 = 2160.0;
const MAX_MM: f64 = 2000.0;

const WIFI_PASSWORD_MIN: usize = 8;
const WIFI_PASSWORD_MAX: usize = 63;

const PAGES: [(&str, &str, &str); 6] = [
    ("/", "Home", "settings.startPageHome"),
    ("/telemetry", "Telemetry", "settings.startPageTelemetry"),
    ("/media", "Media", "settings.startPageMedia"),
    ("/camera", "Camera", "settings.startPageCamera"),
    ("/settings", "Settings", "settings.startPageSettings"),
    ("last", "Last used", "settings.startPageLast"),
];

fn panel_default_option() -> Choice {
    choice(
        Label::new("settings.displayModeDefault", "Panel default"),
        OptionValue::Text(""),
    )
}

fn auto_connector_option() -> Choice {
    choice(
        Label::new("settings.displayConnectorAuto", "Automatic"),
        OptionValue::Text(""),
    )
}

fn touch_page(screen: Screen) -> Node {
    Node::route(
        "touch",
        Label::new("settings.touch", "Touch"),
        vec![Node::custom(
            Label::new("settings.touch", "Touch"),
            "touchCalibration",
            CustomPage::Touch(screen),
        )],
    )
    .icon("touch")
    .disabled(Condition::DisplaysNotSettable)
}

fn display_rows(screen: Screen) -> Vec<Node> {
    let (connector_path, mode_path) = match screen {
        Screen::Main => ("displayConnector.main", "displayMode.main"),
        Screen::Cluster => ("displayConnector.cluster", "displayMode.cluster"),
        Screen::Aux => ("displayConnector.aux", "displayMode.aux"),
    };
    vec![
        Node::select(
            Label::new("settings.displayConnector", "Connector"),
            connector_path,
            vec![auto_connector_option()],
        )
        .icon("displayConnector")
        .display_value()
        .options_from(OptionSource::Connectors)
        .read_only(Condition::DisplaysNotSettable)
        .page(Label::new("settings.displayConnector", "Connector")),
        Node::select(
            Label::new("settings.displayMode", "Display Mode"),
            mode_path,
            vec![panel_default_option()],
        )
        .icon("displayMode")
        .display_value()
        .options_from(OptionSource::DisplayModes(screen))
        .read_only(Condition::DisplaysNotSettable)
        .page(Label::new("settings.displayMode", "Display Mode")),
    ]
}

fn screen_size(
    key: &'static str,
    text: &'static str,
    icon: &'static str,
    path: &'static str,
    min: f64,
    max: f64,
    title: &'static str,
) -> Node {
    Node::number(Label::new(key, text), path)
        .icon(icon)
        .disabled(Condition::DisplaysSettable)
        .range(Some(min), Some(max), Some(1.0))
        .display_value()
        .page(Label::new(key, title))
}

fn screen_size_page(width: &'static str, height: &'static str) -> Node {
    let mm = |key, text, icon, path| {
        Node::number(Label::new(key, text), path)
            .icon(icon)
            .range(Some(0.0), Some(MAX_MM), Some(1.0))
            .display_value()
            .page(Label::new(key, text))
    };
    Node::route(
        "screenSize",
        Label::new("settings.screenSize", "Screen Size"),
        vec![
            mm("settings.screenWidthMm", "Width (mm)", "width", width),
            mm("settings.screenHeightMm", "Height (mm)", "height", height),
        ],
    )
    .icon("screenSize")
}

fn window_size_page(width: Node, height: Node) -> Node {
    Node::route(
        "windowSize",
        Label::new("settings.windowSize", "Window Size"),
        vec![width, height],
    )
    .icon("windowSize")
    .disabled(Condition::DisplaysSettable)
}

fn fullscreen(path: &'static str) -> Node {
    Node::checkbox(Label::new("settings.fullscreen", "Fullscreen"), path).icon("fullscreen")
}

fn screens(main: &'static str, cluster: &'static str, aux: &'static str) -> Vec<Node> {
    vec![
        Node::checkbox(Label::new("settings.mainScreen", "Main"), main).icon("mainScreen"),
        Node::checkbox(Label::new("settings.clusterScreen", "Cluster"), cluster)
            .icon("clusterScreen"),
        Node::checkbox(Label::new("settings.auxScreen", "Aux"), aux).icon("auxScreen"),
    ]
}

fn binding(key: &'static str, text: &'static str, binding: &'static str) -> Node {
    Node::key_binding(Label::new(key, text), "bindings", binding)
}

fn connections() -> Node {
    Node::route(
        "connections",
        Label::new("settings.connections", "Connections"),
        vec![
            Node::text(Label::new("settings.carName", "Car Name"), "carName")
                .icon("carName")
                .display_value()
                .page(Label::new("settings.carName", "Car Name")),
            Node::text(Label::new("settings.uiName", "UI Name"), "oemName")
                .icon("uiName")
                .display_value()
                .page(Label::new("settings.uiName", "UI Name")),
            Node::route(
                "wifi",
                Label::new("settings.wifi", "Wi-Fi"),
                vec![
                    Node::select(
                        Label::new("settings.wifiFrequency", "Wi-Fi Frequency"),
                        "wifiType",
                        vec![],
                    )
                    .icon("wifiFrequency")
                    .display_value()
                    .options_from(OptionSource::WifiBands)
                    .page(Label::new("settings.wifiFrequency", "Wi-Fi Frequency")),
                    Node::text(
                        Label::new("settings.wifiPassword", "Wi-Fi Password"),
                        "wifiPassword",
                    )
                    .icon("wifiPassword")
                    .length(Some(WIFI_PASSWORD_MIN), Some(WIFI_PASSWORD_MAX))
                    .display_value()
                    .page(Label::new("settings.wifiPassword", "Wi-Fi Password")),
                    Node::select(
                        Label::new("settings.wifiChannel", "Wi-Fi Channel"),
                        "wifiChannel",
                        vec![],
                    )
                    .icon("wifiChannel")
                    .display_value()
                    .options_from(OptionSource::WifiChannels)
                    .page(Label::new("settings.wifiChannel", "Wi-Fi Channel")),
                    Node::select(
                        Label::new("settings.wifiChannelWidth", "Wi-Fi Channel Width"),
                        "wifiChannelWidth",
                        vec![
                            number_choice("20 MHz", 20.0),
                            number_choice("40 MHz", 40.0),
                            number_choice("80 MHz", 80.0),
                        ],
                    )
                    .icon("wifiChannel")
                    .display_value()
                    .page(Label::new(
                        "settings.wifiChannelWidth",
                        "Wi-Fi Channel Width",
                    )),
                    Node::select(
                        Label::new("settings.wifiCountry", "Wi-Fi Country"),
                        "country",
                        vec![],
                    )
                    .icon("wifiCountry")
                    .display_value()
                    .options_from(OptionSource::WifiCountries)
                    .page(Label::new("settings.wifiCountry", "Wi-Fi Country")),
                    Node::select(
                        Label::new("settings.wifiInterface", "Wi-Fi Interface"),
                        "wifiInterface",
                        vec![],
                    )
                    .icon("wifiInterface")
                    .display_value()
                    .options_from(OptionSource::WifiInterfaces)
                    .on_pick(OnPick::SwitchDongleOn(Radio::Wifi))
                    .page(Label::new("settings.wifiInterface", "Wi-Fi Interface")),
                    Node::checkbox(
                        Label::new("settings.wifiDedicatedInterface", "Dedicated Interface"),
                        "wifiDedicatedInterface",
                    )
                    .icon("dedicatedInterface")
                    .disabled(Condition::NotLinux),
                    Node::custom(
                        Label::new("settings.wifiLinkSpeed", "Link Speed"),
                        "wifiInterface",
                        CustomPage::WifiLinkInfo,
                    ),
                ],
            )
            .icon("wifi"),
            Node::select(
                Label::new("settings.btAdapter", "Bluetooth Interface"),
                "btAdapter",
                vec![],
            )
            .icon("btInterface")
            .display_value()
            .options_from(OptionSource::BtAdapters)
            .on_pick(OnPick::SwitchDongleOn(Radio::Bt))
            .page(Label::new("settings.btAdapter", "Bluetooth Interface")),
            Node::checkbox(
                Label::new("settings.wirelessAaEnabled", "Wireless Android Auto"),
                "wirelessAaEnabled",
            )
            .icon("wirelessAa"),
            Node::checkbox(
                Label::new("settings.wirelessCpEnabled", "Wireless CarPlay"),
                "wirelessCpEnabled",
            )
            .icon("wirelessCp"),
            Node::checkbox(
                Label::new("settings.autoConnect", "Auto Connect"),
                "autoConn",
            )
            .icon("autoConnect"),
        ],
    )
    .icon("connections")
}

fn screen_settings() -> Node {
    Node::route(
        "screenSettings",
        Label::new("settings.screenSettings", "Screen Settings"),
        vec![
            Node::route(
                "mainScreen",
                Label::new("settings.mainScreen", "Main Screen"),
                [
                    display_rows(Screen::Main),
                    vec![
                        screen_size_page("screenWidthMm.main", "screenHeightMm.main"),
                        window_size_page(
                            screen_size(
                                "settings.mainScreenWidth",
                                "Width",
                                "width",
                                "mainScreenWidth",
                                MIN_WIDTH,
                                MAX_WIDTH,
                                "Main Screen Width",
                            ),
                            screen_size(
                                "settings.mainScreenHeight",
                                "Height",
                                "height",
                                "mainScreenHeight",
                                MIN_HEIGHT,
                                MAX_HEIGHT,
                                "Main Screen Height",
                            ),
                        ),
                        fullscreen("kiosk.main"),
                        touch_page(Screen::Main),
                    ],
                ]
                .concat(),
            )
            .icon("mainScreen"),
            Node::route(
                "clusterScreen",
                Label::new("settings.clusterScreen", "Cluster Screen"),
                [
                    vec![
                        Node::checkbox(
                            Label::new("settings.clusterScreenActive", "Active"),
                            "clusterScreenActive",
                        )
                        .icon("active"),
                    ],
                    display_rows(Screen::Cluster),
                    vec![
                        screen_size_page("screenWidthMm.cluster", "screenHeightMm.cluster"),
                        window_size_page(
                            screen_size(
                                "settings.clusterScreenWidth",
                                "Width",
                                "width",
                                "clusterScreenWidth",
                                MIN_WIDTH,
                                MAX_WIDTH,
                                "Cluster Screen Width",
                            ),
                            screen_size(
                                "settings.clusterScreenHeight",
                                "Height",
                                "height",
                                "clusterScreenHeight",
                                MIN_HEIGHT,
                                MAX_HEIGHT,
                                "Cluster Screen Height",
                            ),
                        ),
                        fullscreen("kiosk.cluster"),
                        touch_page(Screen::Cluster),
                    ],
                ]
                .concat(),
            )
            .icon("clusterScreen"),
            Node::route(
                "auxScreen",
                Label::new("settings.auxScreen", "Aux Screen"),
                [
                    vec![
                        Node::checkbox(
                            Label::new("settings.auxScreenActive", "Active"),
                            "auxScreenActive",
                        )
                        .icon("active"),
                    ],
                    display_rows(Screen::Aux),
                    vec![
                        screen_size_page("screenWidthMm.aux", "screenHeightMm.aux"),
                        window_size_page(
                            screen_size(
                                "settings.auxScreenWidth",
                                "Width",
                                "width",
                                "auxScreenWidth",
                                MIN_WIDTH,
                                MAX_WIDTH,
                                "Aux Screen Width",
                            ),
                            screen_size(
                                "settings.auxScreenHeight",
                                "Height",
                                "height",
                                "auxScreenHeight",
                                MIN_HEIGHT,
                                MAX_HEIGHT,
                                "Aux Screen Height",
                            ),
                        ),
                        fullscreen("kiosk.aux"),
                        touch_page(Screen::Aux),
                    ],
                ]
                .concat(),
            )
            .icon("auxScreen"),
        ],
    )
    .icon("screenSettings")
}

fn tab_settings() -> Node {
    Node::route(
        "tabSettings",
        Label::new("settings.tabSettings", "Tab Settings"),
        vec![
            Node::route(
                "dashboard",
                Label::new("settings.dashboard", "Dashboard"),
                [
                    screens("dashboard.main", "dashboard.cluster", "dashboard.aux"),
                    vec![
                        Node::route(
                            "layouts",
                            Label::new("settings.dashboardLayouts", "Dashboards"),
                            vec![Node::custom(
                                Label::new("settings.dashboardLayouts", "Dashboards"),
                                "dashboardLayouts",
                                CustomPage::DashboardLayouts,
                            )],
                        )
                        .icon("dashboards"),
                    ],
                ]
                .concat(),
            )
            .icon("dashboards"),
            Node::route(
                "media",
                Label::new("settings.media", "Media"),
                screens("media.main", "media.cluster", "media.aux"),
            )
            .icon("media"),
            Node::route(
                "camera",
                Label::new("settings.reverseCamera", "Reverse Camera"),
                [
                    vec![
                        Node::checkbox(
                            Label::new("settings.autoSwitchOnReverse", "Automatic on reverse gear"),
                            "autoSwitchOnReverse",
                        )
                        .icon("autoReverse"),
                    ],
                    screens("camera.main", "camera.cluster", "camera.aux"),
                    vec![
                        Node::checkbox(
                            Label::new("settings.cameraMirror", "Mirror"),
                            "cameraMirror",
                        )
                        .icon("cameraMirror"),
                        Node::select(
                            Label::new("settings.cameraRotation", "Rotation"),
                            "cameraRotation",
                            vec![
                                number_choice("0°", 0.0),
                                number_choice("90°", 90.0),
                                number_choice("180°", 180.0),
                                number_choice("270°", 270.0),
                            ],
                        )
                        .icon("cameraRotation")
                        .display_value()
                        .page(Label::new("settings.cameraRotation", "Camera Rotation")),
                        Node::select(Label::new("settings.camera", "Camera"), "cameraId", vec![])
                            .icon("camera")
                            .display_value()
                            .options_from(OptionSource::Cameras)
                            .page(Label::new("settings.camera", "Camera")),
                    ],
                ]
                .concat(),
            )
            .icon("reverseCamera")
            .display_value(),
        ],
    )
    .icon("tabSettings")
}

fn key_bindings() -> Node {
    Node::route(
        "keyBindings",
        Label::new("settings.keyBindings", "Key Bindings"),
        vec![
            binding("settings.up", "Up", "up"),
            binding("settings.down", "Down", "down"),
            binding("settings.left", "Left", "left"),
            binding("settings.right", "Right", "right"),
            binding("settings.selectUp", "Select Up", "selectUp"),
            binding("settings.selectDown", "Select Down", "selectDown"),
            binding("settings.back", "Back", "back"),
            binding("settings.knobLeft", "Knob Left", "knobLeft"),
            binding("settings.knobRight", "Knob Right", "knobRight"),
            binding("settings.knobUp", "Knob Up", "knobUp"),
            binding("settings.knobDown", "Knob Down", "knobDown"),
            binding("settings.home", "Home", "home"),
            binding("settings.cycleSession", "Cycle Session", "cycleSession"),
            binding("settings.clusterLayout", "Cluster Layout", "clusterLayout"),
            binding("settings.clusterView", "Cluster View", "clusterContent"),
            binding("settings.playPause", "Play/Pause", "playPause"),
            binding("settings.play", "Play", "play"),
            binding("settings.pause", "Pause", "pause"),
            binding("settings.next", "Next", "next"),
            binding("settings.previous", "Previous", "prev"),
            binding("settings.acceptCall", "Accept Call", "acceptPhone"),
            binding("settings.rejectCall", "Reject Call", "rejectPhone"),
            binding("settings.phoneKey0", "Phone Key 0", "phoneKey0"),
            binding("settings.phoneKey1", "Phone Key 1", "phoneKey1"),
            binding("settings.phoneKey2", "Phone Key 2", "phoneKey2"),
            binding("settings.phoneKey3", "Phone Key 3", "phoneKey3"),
            binding("settings.phoneKey4", "Phone Key 4", "phoneKey4"),
            binding("settings.phoneKey5", "Phone Key 5", "phoneKey5"),
            binding("settings.phoneKey6", "Phone Key 6", "phoneKey6"),
            binding("settings.phoneKey7", "Phone Key 7", "phoneKey7"),
            binding("settings.phoneKey8", "Phone Key 8", "phoneKey8"),
            binding("settings.phoneKey9", "Phone Key 9", "phoneKey9"),
            binding("settings.phoneKeyStar", "Phone Key *", "phoneKeyStar"),
            binding("settings.phoneKeyHash", "Phone Key #", "phoneKeyHash"),
            binding(
                "settings.phoneKeyHookSwitch",
                "Hook Switch",
                "phoneKeyHookSwitch",
            ),
            binding(
                "settings.voiceAssistant",
                "Voice Assistant",
                "voiceAssistant",
            ),
            binding(
                "settings.voiceAssistantRelease",
                "Voice Assistant Release",
                "voiceAssistantRelease",
            ),
        ],
    )
    .icon("keyBindings")
}

fn mfi() -> Node {
    Node::route(
        "mfi",
        Label::new("settings.mfi", "MFi"),
        vec![
            Node::number(Label::new("settings.i2cBus", "I2C Bus"), "carPlayMfiI2cBus")
                .icon("i2cBus")
                .range(Some(0.0), Some(20.0), Some(1.0))
                .default_number(2.0)
                .display_value()
                .page(Label::new("settings.i2cBus", "I2C Bus")),
            Node::number(
                Label::new("settings.mfiPowerGpio", "Power Pin"),
                "carPlayMfiPowerGpio",
            )
            .icon("powerPin")
            .range(Some(-1.0), Some(27.0), Some(1.0))
            .default_number(-1.0)
            .display_value()
            .shown(Shown::DashIfNegative)
            .page(Label::new("settings.mfiPowerGpio", "Power Pin")),
            Node::route(
                "mfi-certificate",
                Label::new("settings.mfiCertificate", "Certificate"),
                vec![Node::custom(
                    Label::new("settings.mfiCertificate", "Certificate"),
                    "carPlayMfiFiles",
                    CustomPage::MfiCertificate,
                )],
            )
            .icon("mfiCertificate"),
        ],
    )
    .icon("mfi")
}

fn gps() -> Node {
    Node::route(
        "gps",
        Label::new("settings.gps", "GPS"),
        vec![
            Node::checkbox(Label::new("settings.gpsEnabled", "Enable"), "gpsEnabled")
                .icon("gpsEnabled"),
            Node::text(Label::new("settings.gpsDevice", "Device"), "gpsDevice")
                .icon("gpsDevice")
                .page(Label::new("settings.gpsDevice", "Device")),
            Node::select(
                Label::new("settings.gpsBaudRate", "Baud Rate"),
                "gpsBaudRate",
                vec![
                    number_choice("4800", 4800.0),
                    number_choice("9600", 9600.0),
                    number_choice("19200", 19200.0),
                    number_choice("38400", 38400.0),
                    number_choice("57600", 57600.0),
                    number_choice("115200", 115200.0),
                ],
            )
            .icon("gpsBaudRate")
            .page(Label::new("settings.gpsBaudRate", "Baud Rate")),
            Node::route(
                "info",
                Label::new("settings.gpsInfo", "GPS Data"),
                vec![Node::custom(
                    Label::new("settings.gpsInfo", "GPS Data"),
                    "carName",
                    CustomPage::GpsInfo,
                )],
            )
            .icon("gpsInfo"),
            Node::route(
                "hwinfo",
                Label::new("settings.gpsHwInfo", "HW Info"),
                vec![Node::custom(
                    Label::new("settings.gpsHwInfo", "HW Info"),
                    "carName",
                    CustomPage::GpsHwInfo,
                )],
            )
            .icon("gpsHwInfo"),
        ],
    )
    .icon("gps")
}

const LANGUAGES: [(&str, &str, &str); 32] = [
    ("cs", "Čeština", "Czech"),
    ("da", "Dansk", "Danish"),
    ("de", "Deutsch", "German"),
    ("et", "Eesti", "Estonian"),
    ("en", "English", "English"),
    ("es", "Español", "Spanish"),
    ("fr", "Français", "French"),
    ("ga", "Gaeilge", "Irish"),
    ("hr", "Hrvatski", "Croatian"),
    ("it", "Italiano", "Italian"),
    ("lv", "Latviešu", "Latvian"),
    ("lt", "Lietuvių", "Lithuanian"),
    ("hu", "Magyar", "Hungarian"),
    ("mt", "Malti", "Maltese"),
    ("nl", "Nederlands", "Dutch"),
    ("pl", "Polski", "Polish"),
    ("pt", "Português", "Portuguese"),
    ("ro", "Română", "Romanian"),
    ("sk", "Slovenčina", "Slovak"),
    ("sl", "Slovenščina", "Slovenian"),
    ("fi", "Suomi", "Finnish"),
    ("sv", "Svenska", "Swedish"),
    ("el", "Ελληνικά", "Greek"),
    ("bg", "Български", "Bulgarian"),
    ("uk", "Українська", "Ukrainian"),
    ("he", "עברית", "Hebrew"),
    ("ar", "العربية", "Arabic"),
    ("hi", "हिन्दी", "Hindi"),
    ("th", "ไทย", "Thai"),
    ("ja", "日本語", "Japanese"),
    ("zh", "简体中文", "Chinese"),
    ("ko", "한국어", "Korean"),
];

pub fn schema() -> Node {
    Node::route(
        "general",
        Label::new("settings.general", "General"),
        vec![
            connections(),
            screen_settings(),
            tab_settings(),
            key_bindings(),
            Node::select(
                Label::new("settings.startPage", "Start Page"),
                "startPage",
                PAGES
                    .iter()
                    .map(|&(path, label, key)| {
                        choice(Label::new(key, label), OptionValue::Text(path))
                    })
                    .collect(),
            )
            .icon("startPage")
            .display_value()
            .page(Label::new("settings.startPage", "Start Page")),
            Node::number(
                Label::new("settings.fftDelay", "FFT Delay"),
                "visualAudioDelayMs",
            )
            .icon("fftDelay")
            .display_value()
            .shown(Shown::Suffix(" ms"))
            .page(Label::new("settings.fftDelay", "FFT Visualization Delay")),
            Node::number(Label::new("settings.uiZoom", "UI Zoom"), "uiZoomPercent")
                .icon("uiZoom")
                .display_value()
                .range(Some(50.0), Some(200.0), Some(10.0))
                .shown(Shown::Suffix("%"))
                .page(Label::new("settings.uiZoom", "UI Zoom")),
            Node::select(
                Label::new("settings.language", "Language"),
                "language",
                LANGUAGES
                    .iter()
                    .filter(|(code, _, _)| crate::i18n::translated(code))
                    .map(|&(code, name, english)| {
                        let label = Label::native(name, crate::i18n::script(code), english);
                        choice(label, OptionValue::Text(code))
                    })
                    .collect(),
            )
            .icon("language")
            .display_value()
            .page(Label::new("settings.language", "Language")),
            mfi(),
            crate::settings::schemas::dab::schema(),
            gps(),
        ],
    )
    .icon("general")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_list_matches_the_locales() {
        let listed = LANGUAGES.map(|(code, _, _)| code);
        let locales = crate::i18n::LOCALES.map(|(code, _, _)| code);
        assert_eq!(listed, locales);
    }
}
