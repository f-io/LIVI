// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const SETTINGS: &[(&str, &str)] = &[
    ("carName", "DirectionsCarOutlined"),
    ("uiName", "BadgeOutlined"),
    ("wifi", "WifiOutlined"),
    ("wifiFrequency", "NetworkWifiOutlined"),
    ("wifiPassword", "KeyOutlined"),
    ("wifiChannel", "SettingsInputAntennaOutlined"),
    ("wifiCountry", "PublicOutlined"),
    ("wifiInterface", "RouterOutlined"),
    ("btInterface", "BluetoothOutlined"),
    ("dedicatedInterface", "LanOutlined"),
    ("wirelessAa", "AndroidOutlined"),
    ("wirelessCp", "PhoneIphoneOutlined"),
    ("autoConnect", "AutorenewOutlined"),
    ("mainScreen", "MonitorOutlined"),
    ("clusterScreen", "SpeedOutlined"),
    ("auxScreen", "TvOutlined"),
    ("displayMode", "AspectRatioOutlined"),
    ("displayConnector", "SettingsInputHdmiOutlined"),
    ("touch", "TouchAppOutlined"),
    ("width", "SettingsEthernetOutlined"),
    ("height", "HeightOutlined"),
    ("screenSize", "StraightenOutlined"),
    ("windowSize", "OpenInFull"),
    ("fullscreen", "FullscreenOutlined"),
    ("active", "VisibilityOutlined"),
    ("dashboards", "SpeedOutlined"),
    ("media", "PlayCircleOutlined"),
    ("reverseCamera", "CameraOutlined"),
    ("autoReverse", "UTurnLeftOutlined"),
    ("cameraMirror", "FlipOutlined"),
    ("cameraRotation", "RotateRightOutlined"),
    ("camera", "CameraAltOutlined"),
    ("fps", "ShutterSpeedOutlined"),
    ("dpi", "GrainOutlined"),
    ("viewArea", "CropOutlined"),
    ("safeArea", "CropFreeOutlined"),
    ("drawOutside", "BorderStyleOutlined"),
    ("edgeTop", "VerticalAlignTopOutlined"),
    ("edgeBottom", "VerticalAlignBottomOutlined"),
    ("edgeLeft", "WestOutlined"),
    ("edgeRight", "EastOutlined"),
    ("i2cBus", "CableOutlined"),
    ("powerPin", "BoltOutlined"),
    ("gps", "SatelliteAltOutlined"),
    ("gpsEnabled", "GpsFixedOutlined"),
    ("gpsDevice", "CableOutlined"),
    ("gpsBaudRate", "SpeedOutlined"),
    ("gpsInfo", "SatelliteAltOutlined"),
    ("gpsHwInfo", "DeveloperBoardOutlined"),
    ("devices", "SmartphoneOutlined"),
    ("general", "TuneOutlined"),
    ("car", "DirectionsCarOutlined"),
    ("audio", "VolumeUpOutlined"),
    ("video", "MonitorOutlined"),
    ("appearance", "ContrastOutlined"),
    ("system", "MemoryOutlined"),
    ("connections", "WifiOutlined"),
    ("screenSettings", "DisplaySettingsOutlined"),
    ("tabSettings", "GridViewOutlined"),
    ("keyBindings", "KeyboardOutlined"),
    ("startPage", "HomeOutlined"),
    ("fftDelay", "GraphicEqOutlined"),
    ("carType", "LocalGasStationOutlined"),
    ("steering", "SwapHorizOutlined"),
    ("maxSpeed", "SpeedOutlined"),
    ("speedScaleStep", "LinearScaleOutlined"),
    ("maxRpm", "SpeedOutlined"),
    ("redlineRpm", "WarningAmberOutlined"),
    ("speedUnit", "StraightenOutlined"),
    ("temperatureUnit", "ThermostatOutlined"),
    ("climate", "AcUnitOutlined"),
    ("uiZoom", "ZoomInOutlined"),
    ("language", "TranslateOutlined"),
    ("mfi", "DeveloperBoardOutlined"),
    ("mfiCertificate", "VerifiedUserOutlined"),
    ("carPlayVehicleClimate", "AcUnitOutlined"),
    ("huVolume", "VolumeUpOutlined"),
    ("music", "MusicNoteOutlined"),
    ("navigation", "NavigationOutlined"),
    ("voiceAssistant", "RecordVoiceOverOutlined"),
    ("phoneCalls", "CallOutlined"),
    ("systemSounds", "NotificationsNoneOutlined"),
    ("audioOutput", "SpeakerOutlined"),
    ("audioInput", "MicNoneOutlined"),
    ("samplingFrequency", "EqualizerOutlined"),
    ("volumeLink", "LinkOutlined"),
    ("audioOff", "VolumeOffOutlined"),
    ("dab", "RadioOutlined"),
    ("dabReceiver", "SettingsInputAntennaOutlined"),
    ("dabStations", "CellTowerOutlined"),
    ("dabStationList", "FormatListNumberedOutlined"),
    ("dabGainAuto", "BrightnessAutoOutlined"),
    ("dabGain", "EqualizerOutlined"),
    ("dabPpm", "TuneOutlined"),
    ("mainVideo", "MonitorOutlined"),
    ("clusterVideo", "SpeedOutlined"),
    ("clusterContent", "NavigationOutlined"),
    ("compass", "ExploreOutlined"),
    ("eta", "ScheduleOutlined"),
    ("speedLimit", "SpeedOutlined"),
    ("darkMode", "DarkModeOutlined"),
    ("overlayMessages", "LayersOutlined"),
    ("phoneAppearance", "BrightnessAutoOutlined"),
    ("displayBrightness", "Brightness6Outlined"),
    ("displayBrightnessAuto", "BrightnessAutoOutlined"),
    ("uiColors", "PaletteOutlined"),
    ("contrastGamma", "TonalityOutlined"),
    ("displayColor", "InvertColorsOutlined"),
    ("uiIcon", "ImageOutlined"),
    ("about", "InfoOutlined"),
    ("softwareUpdate", "SystemUpdateAltOutlined"),
    ("logs", "ArticleOutlined"),
    ("debugLogging", "BugReportOutlined"),
    ("restart", "RestartAltOutlined"),
    ("poweroff", "PowerSettingsNewOutlined"),
];

pub fn settings_icon(key: &str) -> Option<&'static str> {
    SETTINGS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_material_names() {
        assert_eq!(settings_icon("audio"), Some("VolumeUpOutlined"));
        assert_eq!(settings_icon("nope"), None);
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = SETTINGS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }
}
