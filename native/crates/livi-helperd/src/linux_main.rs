// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::ExitCode;

use livi_core_proto::helper::HelperSockets;
use livi_helper::embedded::Host;
use livi_helper::{DEFAULT_CHANNEL, DEFAULT_NAME, DEFAULT_PASSPHRASE};
use livi_wifi::{Band, Channel};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn config_path() -> std::path::PathBuf {
    if let Some(dir) =
        std::env::var_os(livi_core_proto::helper::USER_DATA).filter(|d| !d.is_empty())
    {
        return std::path::PathBuf::from(dir).join("config.json");
    }
    let user = std::env::var("SUDO_USER")
        .ok()
        .filter(|u| !u.is_empty() && u != "root")
        .or_else(|| std::env::var("USER").ok());
    let home = match user {
        Some(u) if u != "root" => format!("/home/{u}"),
        _ => std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
    };
    std::path::Path::new(&home).join(".config/LIVI/config.json")
}

pub struct DeviceConfig {
    json: serde_json::Value,
}

impl DeviceConfig {
    pub fn load() -> Self {
        let json = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        Self { json }
    }

    pub fn string(&self, json_key: &str, env_key: &str, default: &str) -> String {
        if let Some(s) = self.json.get(json_key).and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return s.to_string();
        }
        std::env::var(env_key)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    pub fn int<T: std::str::FromStr + std::convert::TryFrom<i64>>(
        &self,
        json_key: &str,
        env_key: &str,
        default: T,
    ) -> T {
        if let Some(n) = self.json.get(json_key).and_then(|v| v.as_i64())
            && let Ok(v) = T::try_from(n)
        {
            return v;
        }
        env_or(env_key, default)
    }
}

fn configured_channel(dc: &DeviceConfig) -> Channel {
    let band = Band::of_setting(&dc.string("wifiType", "LIVI_WIFI_TYPE", "5ghz"));
    Channel::of_setting(
        band.unwrap_or(Band::Ghz5),
        dc.int("wifiChannel", "LIVI_CHANNEL", DEFAULT_CHANNEL),
    )
}

fn ap_iface(dc: &DeviceConfig) -> String {
    livi_helper::bluez::ap_iface(&dc.string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0"))
}

pub fn host() -> Host {
    let config = config_path();
    Host {
        sockets: HelperSockets::system(),
        mfi: config.with_file_name("mfi"),
        lockdown: crate::lockdown_dir(),
        bt_keys: config.with_file_name("bluetooth-keys"),
    }
}

pub fn run_wifi_ap() -> ExitCode {
    let dc = DeviceConfig::load();
    let cfg = livi_helper_stack::wifi_ap::ApConfig {
        iface: ap_iface(&dc),
        ssid: dc.string("carName", "LIVI_CP_NAME", DEFAULT_NAME),
        passphrase: dc.string("wifiPassword", "LIVI_PASSPHRASE", DEFAULT_PASSPHRASE),
        channel: configured_channel(&dc),
        width: dc.int("wifiChannelWidth", "LIVI_CHANNEL_WIDTH", 40u32),
        country: dc.string("country", "LIVI_COUNTRY", "DE"),
        ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
    };
    livi_helper_stack::wifi_ap::run(cfg)
}

pub fn run_wifi_ap_status() -> ExitCode {
    let dc = DeviceConfig::load();
    print!("{}", livi_helper_stack::wifi_ap::status(&ap_iface(&dc)));
    ExitCode::SUCCESS
}

fn installed(
    what: &str,
    result: Result<(), livi_helper_stack::privileged::PrivilegedError>,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[{what}] install failed: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn run_install_wifi_ap(unit: Option<String>, rule: Option<String>) -> ExitCode {
    let (Some(unit), Some(rule)) = (unit, rule) else {
        eprintln!("[wifi-ap] usage: --install-wifi-ap <unit file> <sudoers file>");
        return ExitCode::FAILURE;
    };
    installed(
        "wifi-ap",
        livi_helper_stack::privileged::install_wifi_ap(&unit, &rule),
    )
}

pub fn run_install_udev_rule(rule: Option<String>, filter: Option<String>) -> ExitCode {
    let Some(rule) = rule else {
        eprintln!("[udev] usage: --install-udev-rule <rule file> [<touch filter>]");
        return ExitCode::FAILURE;
    };
    installed(
        "udev",
        livi_helper_stack::privileged::install_udev_rule(&rule, filter.as_deref()),
    )
}

pub fn run_install_kiosk_unit(unit: Option<String>) -> ExitCode {
    let Some(unit) = unit else {
        eprintln!("[kiosk] usage: --install-kiosk-unit <unit file>");
        return ExitCode::FAILURE;
    };
    installed(
        "kiosk",
        livi_helper_stack::privileged::install_kiosk_unit(&unit),
    )
}

pub fn run_install_gvfs_guard(script: Option<String>, rule: Option<String>) -> ExitCode {
    let (Some(script), Some(rule)) = (script, rule) else {
        eprintln!("[gvfs] usage: --install-gvfs-guard <script file> <sudoers file>");
        return ExitCode::FAILURE;
    };
    installed(
        "gvfs",
        livi_helper_stack::privileged::install_gvfs_guard(&script, &rule),
    )
}

pub fn run_wifi_ap_claim() -> ExitCode {
    let dc = DeviceConfig::load();
    livi_helper_stack::wifi_ap::release_iface_from_nm(&ap_iface(&dc));
    ExitCode::SUCCESS
}

pub fn run_wifi_ap_teardown() -> ExitCode {
    let iface = livi_helper_stack::wifi_ap::unmanaged_iface().unwrap_or_else(|| {
        DeviceConfig::load().string("wifiInterface", "LIVI_WIFI_IFACE", "wlan0")
    });
    livi_helper_stack::wifi_ap::teardown(&iface);
    ExitCode::SUCCESS
}

pub fn run_bt_tunnel() -> ExitCode {
    match livi_link_host::bt::tunnel(&|_| {}) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[bt] {e}");
            ExitCode::FAILURE
        }
    }
}
