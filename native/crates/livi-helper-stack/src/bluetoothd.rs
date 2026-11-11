// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const BT_CLASS: &str = "0x200418";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wireless {
    pub carplay: bool,
    pub android_auto: bool,
}

impl Wireless {
    pub fn any(self) -> bool {
        self.carplay || self.android_auto
    }

    pub fn disabled_plugins(self) -> Vec<&'static str> {
        let mut plugins = Vec::new();
        if self.android_auto {
            plugins.push("sap");
        }
        if self.carplay {
            plugins.push("midi");
        }
        plugins
    }
}

fn is_class_line(line: &str) -> bool {
    let cleaned = line.trim().trim_start_matches('#').trim();
    cleaned.starts_with("Class") && cleaned.contains('=')
}

fn is_livis_class(line: &str) -> bool {
    !line.trim().starts_with('#')
        && is_class_line(line)
        && line
            .split_once('=')
            .is_some_and(|(_, v)| v.trim().eq_ignore_ascii_case(BT_CLASS))
}

pub fn class_line(conf: &str) -> Option<String> {
    let mut in_general = false;
    for line in conf.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            in_general = stripped == "[General]";
        } else if in_general && is_class_line(line) {
            return Some(line.to_string());
        }
    }
    None
}

pub fn without_class(current: &str, original: Option<&str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_general = false;
    for line in current.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            in_general = stripped == "[General]";
        } else if in_general && is_livis_class(line) {
            out.extend(original);
            continue;
        }
        out.push(line);
    }
    let mut new = out.join("\n");
    if current.ends_with('\n') {
        new.push('\n');
    }
    new
}

pub fn with_class(original: &str) -> String {
    let desired = format!("Class = {BT_CLASS}");
    let mut out: Vec<String> = Vec::new();
    let mut in_general = false;
    let mut seen = false;
    for line in original.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            if in_general && !seen {
                out.push(desired.clone());
                seen = true;
            }
            in_general = stripped == "[General]";
            out.push(line.to_string());
            continue;
        }
        if in_general {
            let cleaned = stripped.trim_start_matches('#').trim();
            if cleaned.starts_with("Class") && cleaned.contains('=') {
                if !seen {
                    out.push(desired.clone());
                    seen = true;
                }
                continue;
            }
        }
        out.push(line.to_string());
    }
    if in_general && !seen {
        out.push(desired.clone());
        seen = true;
    }
    if !seen {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[General]".into());
        out.push(desired);
    }
    let mut new = out.join("\n");
    if !new.ends_with('\n') {
        new.push('\n');
    }
    new
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Wireless, class_line, is_livis_class, with_class, without_class};
    use std::path::Path;

    const DROPIN_DIR: &str = "/etc/systemd/system/bluetooth.service.d";
    const DROPIN_CFG: &str = "/etc/systemd/system/bluetooth.service.d/livi-no-sap.conf";
    const MAIN_CONF: &str = "/etc/bluetooth/main.conf";
    const CLASS_BEFORE: &str = "/var/lib/livi/main-conf-class";

    fn find_bluetoothd() -> Option<String> {
        [
            "/usr/libexec/bluetooth/bluetoothd",
            "/usr/lib/bluetooth/bluetoothd",
            "/usr/sbin/bluetoothd",
        ]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .map(str::to_string)
    }

    fn remove_stale_dropins() -> bool {
        let mut changed = false;
        if let Ok(entries) = std::fs::read_dir(DROPIN_DIR) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("livi-")
                    && name.ends_with(".conf")
                    && entry.path() != Path::new(DROPIN_CFG)
                {
                    changed |= std::fs::remove_file(entry.path()).is_ok();
                }
            }
        }
        changed
    }

    fn write_noplugin_dropin(plugins: &[&str]) -> bool {
        let mut changed = remove_stale_dropins();
        if plugins.is_empty() {
            return changed | std::fs::remove_file(DROPIN_CFG).is_ok();
        }
        let Some(bluetoothd) = find_bluetoothd() else {
            eprintln!("[helperd] bluetoothd binary not found, skipping --noplugin setup");
            return changed;
        };
        let content = format!(
            "[Service]\nExecStart=\nExecStart={bluetoothd} --noplugin={}\n",
            plugins.join(",")
        );
        let current = std::fs::read_to_string(DROPIN_CFG).unwrap_or_default();
        if current != content {
            let _ = std::fs::create_dir_all(DROPIN_DIR);
            match std::fs::write(DROPIN_CFG, content) {
                Ok(()) => changed = true,
                Err(e) => eprintln!("[helperd] could not write {DROPIN_CFG}: {e}"),
            }
        }
        changed
    }

    fn write_main_conf(original: &str, new: String) -> bool {
        if new == original {
            return false;
        }
        match std::fs::write(MAIN_CONF, new) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[helperd] could not write {MAIN_CONF}: {e}");
                false
            }
        }
    }

    fn ensure_main_conf_class() -> bool {
        let Ok(original) = std::fs::read_to_string(MAIN_CONF) else {
            return false;
        };
        if !Path::new(CLASS_BEFORE).exists() {
            let before = class_line(&original)
                .filter(|l| !is_livis_class(l))
                .unwrap_or_default();
            let _ = std::fs::create_dir_all("/var/lib/livi");
            if let Err(e) = std::fs::write(CLASS_BEFORE, before) {
                eprintln!("[helperd] could not write {CLASS_BEFORE}, the class stays: {e}");
            }
        }
        write_main_conf(&original, with_class(&original))
    }

    fn restore_main_conf_class(dropin_was_ours: bool) -> bool {
        let Ok(current) = std::fs::read_to_string(MAIN_CONF) else {
            return false;
        };
        let before = match std::fs::read_to_string(CLASS_BEFORE) {
            Ok(before) => Some(before),
            Err(_) if dropin_was_ours => None,
            Err(_) => return false,
        };
        let original = before
            .as_deref()
            .map(str::trim_end)
            .filter(|l| !l.is_empty());
        let changed = write_main_conf(&current, without_class(&current, original));
        let _ = std::fs::remove_file(CLASS_BEFORE);
        changed
    }

    pub fn setup(wireless: Wireless) {
        let dropin_was_ours = Path::new(DROPIN_CFG).exists();
        let plugins = wireless.disabled_plugins();
        let mut changed = write_noplugin_dropin(&plugins);
        changed |= if wireless.any() {
            ensure_main_conf_class()
        } else {
            restore_main_conf_class(dropin_was_ours)
        };
        if changed {
            println!(
                "[helperd] restarting bluetoothd (--noplugin={}, class {})",
                plugins.join(","),
                if wireless.any() {
                    "car kit"
                } else {
                    "as it was"
                }
            );
            let _ = std::process::Command::new(crate::sys::tool("systemctl"))
                .arg("daemon-reload")
                .status();
            let _ = std::process::Command::new(crate::sys::tool("systemctl"))
                .args(["restart", "bluetooth"])
                .status();
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::setup;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_an_existing_class() {
        let conf = "[General]\nClass = 0x000000\nName = x\n";
        assert_eq!(with_class(conf), "[General]\nClass = 0x200418\nName = x\n");
    }

    #[test]
    fn uncomments_a_commented_class() {
        let conf = "[General]\n#Class = 0x000100\n";
        assert_eq!(with_class(conf), "[General]\nClass = 0x200418\n");
    }

    #[test]
    fn adds_class_to_an_existing_general_section() {
        let conf = "[General]\nName = x\n\n[Policy]\n";
        assert_eq!(
            with_class(conf),
            "[General]\nName = x\n\nClass = 0x200418\n[Policy]\n"
        );
    }

    #[test]
    fn appends_a_general_section_when_missing() {
        let conf = "[Policy]\nAutoEnable=true\n";
        assert_eq!(
            with_class(conf),
            "[Policy]\nAutoEnable=true\n\n[General]\nClass = 0x200418\n"
        );
    }

    #[test]
    fn leaves_a_correct_conf_unchanged() {
        let conf = "[General]\nClass = 0x200418\n";
        assert_eq!(with_class(conf), conf);
    }

    #[test]
    fn each_protocol_turns_off_only_the_plugin_in_its_way() {
        let w = |carplay, android_auto| {
            Wireless {
                carplay,
                android_auto,
            }
            .disabled_plugins()
        };
        assert!(w(false, false).is_empty());
        assert_eq!(w(true, false), ["midi"]);
        assert_eq!(w(false, true), ["sap"]);
        assert_eq!(w(true, true), ["sap", "midi"]);
        assert!(!Wireless::default().any());
    }

    #[test]
    fn the_class_line_is_found_in_general_only() {
        assert_eq!(
            class_line("[General]\n#Class = 0x000100\n").as_deref(),
            Some("#Class = 0x000100")
        );
        assert_eq!(class_line("[Policy]\nClass = 0x1\n"), None);
        assert_eq!(class_line("[General]\nName = x\n"), None);
    }

    #[test]
    fn livis_class_goes_back_to_what_was_there() {
        let conf = with_class("[General]\n#Class = 0x000100\nName = x\n");
        assert_eq!(
            without_class(&conf, Some("#Class = 0x000100")),
            "[General]\n#Class = 0x000100\nName = x\n"
        );
    }

    #[test]
    fn livis_class_goes_when_there_was_none() {
        let original = "[General]\nName = x\n\n[Policy]\n";
        assert_eq!(
            without_class(&with_class(original), None),
            "[General]\nName = x\n\n[Policy]\n"
        );
    }

    #[test]
    fn a_class_someone_else_set_stays() {
        let conf = "[General]\nClass = 0x000100\n";
        assert_eq!(without_class(conf, None), conf);
        let commented = "[General]\n#Class = 0x200418\n";
        assert_eq!(without_class(commented, None), commented);
    }
}
