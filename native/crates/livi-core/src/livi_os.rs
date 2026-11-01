// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The immutable LIVI OS image sets `LIVI_OS=1`. Its root file system is read-only and ships
//! every privileged file LIVI would otherwise install itself, so under it LIVI uses the fixed
//! interface the image provides. docs/LIVI-OS.md is the contract.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;

use livi_core_proto::LIFELINE_ENV;

pub const ENV: &str = "LIVI_OS";
pub const HELPER_ENV: &str = "LIVI_OS_HELPER";
pub const DEFAULT_HELPER: &str = "/usr/libexec/livi-helperd";
pub const CAN_CAPTURE: &str = "/usr/libexec/livi-can-capture";
/// Written to `resources/livi-os-helper-api` in the AppImage, bumped with every change of
/// docs/LIVI-OS.md the image has to follow.
pub const CONTRACT: &str = "2";
const PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// What `helper::helper_env` hands the helper, all of it reaches it through `env_keep`.
pub const HELPER_KEYS: [&str; 20] = [
    LIFELINE_ENV,
    "DEBUG",
    "LIVI_AA_WIRELESS",
    "LIVI_CP_WIRELESS",
    "LIVI_CP_NAME",
    "LIVI_CP_FUELS",
    "LIVI_CP_CONNECTORS",
    "LIVI_CP_THEME",
    "LIVI_BT_ADAPTER",
    "LIVI_MFI_FILES",
    "LIVI_WIFI_IFACE",
    "LIVI_PASSPHRASE",
    "LIVI_WIFI_TYPE",
    "LIVI_CHANNEL",
    "LIVI_COUNTRY",
    "LIVI_CP_DEBUG",
    "LIVI_CP_DEVICE_ID",
    "LIVI_CP_PK",
    "LIVI_CP_PI",
    "LIVI_CP_AIRPLAY_PORT",
];

/// Developer knobs the helper reads that LIVI does not set, taken over from LIVI's own
/// environment when they are there.
pub const PASSTHROUGH_KEYS: [&str; 12] = [
    "LIVI_AP_IP",
    "LIVI_PORT",
    "LIVI_DONGLE",
    "LIVI_CP_WIRED",
    "LIVI_AA_USB",
    "LIVI_BT_VIA_DONGLE",
    "LIVI_CHANNEL_WIDTH",
    "LIVI_CP_USB_IFACE",
    "LIVI_LOCKDOWN_STORE",
    "LIVI_CP_MFI_POWER_GPIO",
    "LIVI_CP_AVAILABLE_CURRENT_MA",
    "LIVI_CP_SOURCE_VERSION",
];

fn active_in(value: Option<&str>) -> bool {
    cfg!(target_os = "linux") && value == Some("1")
}

pub fn active() -> bool {
    active_in(std::env::var(ENV).ok().as_deref())
}

fn helper_in(over: Option<OsString>) -> PathBuf {
    over.filter(|p| !p.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_HELPER), PathBuf::from)
}

pub fn helper() -> PathBuf {
    helper_in(std::env::var_os(HELPER_ENV))
}

/// True under LIVI OS (`active`), where `what` comes with the image. Says so once per `what`.
pub fn provides_in(active: bool, what: &'static str) -> bool {
    static NOTED: Mutex<BTreeSet<&str>> = Mutex::new(BTreeSet::new());
    if !active {
        return false;
    }
    if NOTED.lock().unwrap_or_else(|e| e.into_inner()).insert(what) {
        println!("[livi-os] {what} comes with the image, not installed");
    }
    true
}

/// The whole environment of `sudo -n <helper>`: PATH, then only allow-listed keys.
pub fn helper_env(
    wanted: Vec<(String, String)>,
    outer: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut env = vec![("PATH".to_string(), PATH.to_string())];
    env.extend(
        PASSTHROUGH_KEYS
            .iter()
            .filter(|k| !wanted.iter().any(|(w, _)| w == *k))
            .filter_map(|k| outer(k).map(|v| (k.to_string(), v))),
    );
    env.extend(wanted.into_iter().filter(|(k, _)| allowed(k)));
    env
}

fn allowed(key: &str) -> bool {
    HELPER_KEYS.contains(&key) || PASSTHROUGH_KEYS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_one_on_linux_means_livi_os() {
        assert_eq!(active_in(Some("1")), cfg!(target_os = "linux"));
        assert!(!active_in(Some("0")));
        assert!(!active_in(Some("")));
        assert!(!active_in(Some("true")));
        assert!(!active_in(None));
    }

    #[test]
    fn the_helper_sits_at_its_fixed_path_unless_overridden() {
        assert_eq!(helper_in(None), PathBuf::from("/usr/libexec/livi-helperd"));
        assert_eq!(helper_in(Some("".into())), PathBuf::from(DEFAULT_HELPER));
        assert_eq!(
            helper_in(Some("/opt/livi/helperd".into())),
            PathBuf::from("/opt/livi/helperd")
        );
    }

    #[test]
    fn nothing_is_provided_off_livi_os() {
        assert!(!provides_in(false, "the udev rule"));
        assert!(provides_in(true, "the udev rule"));
        assert!(provides_in(true, "the udev rule"));
    }

    #[test]
    fn the_helper_sees_path_and_the_allow_list_only() {
        let wanted = vec![
            ("LIVI_CP_PK".to_string(), "ab".to_string()),
            ("LIVI_SOMETHING_NEW".to_string(), "x".to_string()),
            ("LIVI_PORT".to_string(), "5277".to_string()),
        ];
        let outer = |k: &str| match k {
            "LIVI_AP_IP" => Some("10.0.0.1".to_string()),
            "LIVI_PORT" => Some("1".to_string()),
            "HOME" => Some("/home/pi".to_string()),
            _ => None,
        };
        let env = helper_env(wanted, outer);
        assert_eq!(
            env,
            vec![
                ("PATH".to_string(), PATH.to_string()),
                ("LIVI_AP_IP".to_string(), "10.0.0.1".to_string()),
                ("LIVI_CP_PK".to_string(), "ab".to_string()),
                ("LIVI_PORT".to_string(), "5277".to_string()),
            ]
        );
    }

    #[test]
    fn the_contract_doc_names_every_key_and_the_version() {
        let doc = include_str!("../../../../docs/LIVI-OS.md");
        for key in HELPER_KEYS.iter().chain(PASSTHROUGH_KEYS.iter()) {
            assert!(doc.contains(key), "docs/LIVI-OS.md misses {key}");
        }
        assert!(doc.contains(DEFAULT_HELPER));
        assert!(doc.contains(CAN_CAPTURE));
        assert!(doc.contains(&format!("contract version `{CONTRACT}`")));
    }

    #[test]
    fn the_appimage_marker_names_this_contract() {
        let marker = include_str!("../../../../assets/linux/livi-os-helper-api");
        assert_eq!(marker.trim(), CONTRACT);
    }
}
