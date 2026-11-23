// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::ExitCode;

use livi_core_proto::helper::HelperSockets;
use livi_helper::in_process::Host;
use livi_wifi::{Band, Channel};
use serde_json::Value;

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

/// The config file with core's defaults behind it, so a missing or empty key reads as core reads it.
struct DeviceConfig {
    file: Value,
    defaults: Value,
}

impl DeviceConfig {
    fn load() -> Self {
        Self {
            file: std::fs::read_to_string(config_path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null),
            defaults: serde_json::to_value(livi_core_proto::config::defaults())
                .unwrap_or(Value::Null),
        }
    }

    fn text(&self, key: &str) -> String {
        [&self.file, &self.defaults]
            .into_iter()
            .find_map(|v| v.get(key)?.as_str().filter(|s| !s.is_empty()))
            .unwrap_or_default()
            .to_string()
    }

    fn number(&self, key: &str) -> u32 {
        [&self.file, &self.defaults]
            .into_iter()
            .find_map(|v| u32::try_from(v.get(key)?.as_u64()?).ok())
            .unwrap_or_default()
    }

    fn channel(&self) -> Channel {
        let band = Band::of_setting(&self.text("wifiType")).unwrap_or(Band::Ghz5);
        Channel::of_setting(band, self.number("wifiChannel"))
    }

    fn ap_iface(&self) -> String {
        livi_helper::ap_iface(&self.text("wifiInterface"))
    }
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
        iface: dc.ap_iface(),
        ssid: dc.text("carName"),
        passphrase: dc.text("wifiPassword"),
        channel: dc.channel(),
        width: dc.number("wifiChannelWidth"),
        country: dc.text("country"),
        ap_ip: std::env::var("LIVI_AP_IP").unwrap_or_else(|_| "10.10.0.1".into()),
    };
    livi_helper_stack::wifi_ap::run(cfg)
}

pub fn run_wifi_ap_status() -> ExitCode {
    let dc = DeviceConfig::load();
    print!("{}", livi_helper_stack::wifi_ap::status(&dc.ap_iface()));
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
    livi_helper_stack::wifi_ap::release_iface_from_nm(&DeviceConfig::load().ap_iface());
    ExitCode::SUCCESS
}

pub fn run_wifi_ap_teardown() -> ExitCode {
    let iface = livi_helper_stack::wifi_ap::unmanaged_iface()
        .unwrap_or_else(|| DeviceConfig::load().text("wifiInterface"));
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
