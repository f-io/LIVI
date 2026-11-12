// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

const HELPER_DIR: &str = "/tmp";
pub const USER_DATA: &str = "LIVI_USER_DATA";

const AA_WIRELESS: &str = "LIVI_AA_WIRELESS";
const CP_WIRELESS: &str = "LIVI_CP_WIRELESS";
const DEBUG: &str = "DEBUG";
const CP_DEBUG: &str = "LIVI_CP_DEBUG";
const CAR_NAME: &str = "LIVI_CP_NAME";
const FUELS: &str = "LIVI_CP_FUELS";
const CONNECTORS: &str = "LIVI_CP_CONNECTORS";
const THEME: &str = "LIVI_CP_THEME";
const BT_ADAPTER: &str = "LIVI_BT_ADAPTER";
const MFI_FILES: &str = "LIVI_MFI_FILES";
const WIFI_IFACE: &str = "LIVI_WIFI_IFACE";
const PASSPHRASE: &str = "LIVI_PASSPHRASE";
const WIFI_TYPE: &str = "LIVI_WIFI_TYPE";
const CHANNEL: &str = "LIVI_CHANNEL";
const COUNTRY: &str = "LIVI_COUNTRY";
const SOURCE_VERSION: &str = "LIVI_CP_SOURCE_VERSION";
const MFI_I2C_BUS: &str = "LIVI_CP_MFI_I2C_BUS";
const MFI_POWER_GPIO: &str = "LIVI_CP_MFI_POWER_GPIO";
const MFI_I2C_BUS_DEFAULT: u32 = 2;
const MFI_POWER_GPIO_DEFAULT: i32 = -1;
const DEVICE_ID: &str = "LIVI_CP_DEVICE_ID";
const PUBLIC_KEY: &str = "LIVI_CP_PK";
const PAIRING_ID: &str = "LIVI_CP_PI";
const AIRPLAY_PORT: &str = "LIVI_CP_AIRPLAY_PORT";
const MIRROR_PORT: &str = "LIVI_AIRPLAY_PORT";
const MIRROR_TXT: &str = "LIVI_AIRPLAY_TXT";
const MIRROR_NAME: &str = "LIVI_AIRPLAY_NAME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperSockets {
    pub carplay: PathBuf,
    pub android_auto: PathBuf,
    pub shared: PathBuf,
    pub sco_mic: PathBuf,
    pub sessions: PathBuf,
}

impl HelperSockets {
    pub fn within(dir: &Path) -> Self {
        Self {
            carplay: dir.join("cp-bt.sock"),
            android_auto: dir.join("aa-bt.sock"),
            shared: dir.join("livi-shared.sock"),
            sco_mic: dir.join("aa-sco.mic"),
            sessions: dir.to_path_buf(),
        }
    }

    pub fn system() -> Self {
        Self::within(Path::new(HELPER_DIR))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperSettings {
    pub wireless_android_auto: bool,
    pub wireless_carplay: bool,
    pub debug: bool,
    pub car_name: String,
    pub fuels: String,
    pub connectors: String,
    pub theme: String,
    pub bt_adapter: String,
    pub mfi_files: String,
    pub wifi_iface: String,
    pub passphrase: String,
    pub wifi_type: String,
    pub channel: u32,
    pub country: String,
    pub source_version: String,
    pub mfi_i2c_bus: u32,
    pub mfi_power_gpio: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperStart {
    pub settings: HelperSettings,
    pub device_id: String,
    pub public_key: String,
    pub pairing_id: String,
    pub airplay_port: Option<u16>,
    pub mirror: Option<Mirror>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    pub name: String,
    pub port: u16,
    pub txt: Vec<String>,
}

pub fn mirror_name(car_name: &str) -> String {
    match car_name.trim() {
        "" => "LIVI".into(),
        name => name.into(),
    }
}

impl Default for HelperSettings {
    fn default() -> Self {
        Self {
            wireless_android_auto: true,
            wireless_carplay: false,
            debug: false,
            car_name: String::new(),
            fuels: String::new(),
            connectors: String::new(),
            theme: String::new(),
            bt_adapter: String::new(),
            mfi_files: String::new(),
            wifi_iface: String::new(),
            passphrase: String::new(),
            wifi_type: String::new(),
            channel: 0,
            country: String::new(),
            source_version: String::new(),
            mfi_i2c_bus: MFI_I2C_BUS_DEFAULT,
            mfi_power_gpio: MFI_POWER_GPIO_DEFAULT,
        }
    }
}

fn flag(on: bool, off: &str) -> String {
    if on { "1".into() } else { off.into() }
}

impl HelperStart {
    pub fn to_env(&self) -> Vec<(String, String)> {
        let s = &self.settings;
        let channel = if s.channel == 0 {
            String::new()
        } else {
            s.channel.to_string()
        };
        let mut env: Vec<(&str, String)> = vec![
            (AA_WIRELESS, flag(s.wireless_android_auto, "0")),
            (CP_WIRELESS, flag(s.wireless_carplay, "")),
            (DEBUG, flag(s.debug, "")),
            (CAR_NAME, s.car_name.clone()),
            (FUELS, s.fuels.clone()),
            (CONNECTORS, s.connectors.clone()),
            (THEME, s.theme.clone()),
            (BT_ADAPTER, s.bt_adapter.clone()),
            (MFI_FILES, s.mfi_files.clone()),
            (WIFI_IFACE, s.wifi_iface.clone()),
            (PASSPHRASE, s.passphrase.clone()),
            (WIFI_TYPE, s.wifi_type.clone()),
            (CHANNEL, channel),
            (COUNTRY, s.country.clone()),
            (SOURCE_VERSION, s.source_version.clone()),
            (MFI_I2C_BUS, s.mfi_i2c_bus.to_string()),
            (MFI_POWER_GPIO, s.mfi_power_gpio.to_string()),
            (CP_DEBUG, flag(s.debug, "")),
            (DEVICE_ID, self.device_id.clone()),
            (PUBLIC_KEY, self.public_key.clone()),
            (PAIRING_ID, self.pairing_id.clone()),
        ];
        if let Some(port) = self.airplay_port.filter(|p| *p > 0) {
            env.push((AIRPLAY_PORT, port.to_string()));
        }
        if let Some(mirror) = &self.mirror {
            env.push((MIRROR_NAME, mirror.name.clone()));
            env.push((MIRROR_PORT, mirror.port.to_string()));
            env.push((MIRROR_TXT, mirror.txt.join("\n")));
        }
        env.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    pub fn from_env() -> Self {
        Self::from_vars(|key| std::env::var(key).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let text = |key| var(key).unwrap_or_default();
        Self {
            settings: HelperSettings {
                wireless_android_auto: var(AA_WIRELESS).is_none_or(|v| v != "0"),
                wireless_carplay: var(CP_WIRELESS).is_some_and(|v| v == "1"),
                debug: var(DEBUG).is_some_and(|v| v == "1"),
                car_name: text(CAR_NAME),
                fuels: text(FUELS),
                connectors: text(CONNECTORS),
                theme: text(THEME),
                bt_adapter: text(BT_ADAPTER),
                mfi_files: text(MFI_FILES),
                wifi_iface: text(WIFI_IFACE),
                passphrase: text(PASSPHRASE),
                wifi_type: text(WIFI_TYPE),
                channel: var(CHANNEL).and_then(|v| v.parse().ok()).unwrap_or(0),
                country: text(COUNTRY),
                source_version: text(SOURCE_VERSION),
                mfi_i2c_bus: var(MFI_I2C_BUS)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(MFI_I2C_BUS_DEFAULT),
                mfi_power_gpio: var(MFI_POWER_GPIO)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(MFI_POWER_GPIO_DEFAULT),
            },
            device_id: text(DEVICE_ID),
            public_key: text(PUBLIC_KEY),
            pairing_id: text(PAIRING_ID),
            airplay_port: var(AIRPLAY_PORT)
                .and_then(|v| v.parse().ok())
                .filter(|p| *p > 0),
            mirror: var(MIRROR_PORT)
                .and_then(|v| v.parse().ok())
                .filter(|p| *p > 0)
                .map(|port| Mirror {
                    name: mirror_name(&text(MIRROR_NAME)),
                    port,
                    txt: text(MIRROR_TXT).lines().map(str::to_string).collect(),
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> HelperStart {
        HelperStart {
            settings: HelperSettings {
                wireless_android_auto: false,
                wireless_carplay: true,
                debug: true,
                car_name: "LIVI".into(),
                fuels: "Gasoline,Electric".into(),
                connectors: "Combo2:150000".into(),
                theme: "44C9FA31EC:985:19.0".into(),
                bt_adapter: "hci1".into(),
                mfi_files: "files".into(),
                wifi_iface: "wlan1".into(),
                passphrase: "12345678".into(),
                wifi_type: "2.4ghz".into(),
                channel: 6,
                country: "DE".into(),
                source_version: "950.7.1".into(),
                mfi_i2c_bus: 1,
                mfi_power_gpio: 21,
            },
            device_id: "88:00:33:77:8C:26".into(),
            public_key: "ab".repeat(32),
            pairing_id: "pi-1".into(),
            airplay_port: Some(7000),
            mirror: Some(Mirror {
                name: "Golf".into(),
                port: 7100,
                txt: vec!["deviceid=88:00:33:77:8C:27".into(), "acl=0".into()],
            }),
        }
    }

    fn back(env: &[(String, String)]) -> HelperStart {
        HelperStart::from_vars(|key| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
    }

    #[test]
    fn the_environment_carries_the_start_unchanged() {
        let start = start();
        assert_eq!(back(&start.to_env()), start);
        let quiet = HelperStart {
            settings: HelperSettings::default(),
            airplay_port: None,
            mirror: None,
            ..start
        };
        assert_eq!(back(&quiet.to_env()), quiet);
    }

    #[test]
    fn an_empty_environment_keeps_wireless_android_auto_on() {
        let start = back(&[]);
        assert_eq!(start.settings, HelperSettings::default());
        assert_eq!(start.airplay_port, None);
        assert_eq!(start.mirror, None);
    }

    #[test]
    fn the_sockets_share_one_directory() {
        let sockets = HelperSockets::within(Path::new("/run/livi"));
        assert_eq!(sockets.carplay, PathBuf::from("/run/livi/cp-bt.sock"));
        assert_eq!(
            HelperSockets::system().sco_mic,
            PathBuf::from("/tmp/aa-sco.mic")
        );
    }
}
