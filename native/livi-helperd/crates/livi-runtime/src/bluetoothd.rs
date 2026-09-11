// bluetoothd host setup: --noplugin=sap,midi (sap squats RFCOMM channel 8, the AA slot;
// BLE MIDI takes a 128-bit UUID slot in the CarPlay EIR) and the car-kit device class.

const BT_CLASS: &str = "0x200418";

/// Returns `original` with `Class = 0x200418` in `[General]`, adding line or section as needed.
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
    use super::with_class;
    use std::path::Path;

    const DISABLED_PLUGINS: &str = "sap,midi";

    const DROPIN_DIR: &str = "/etc/systemd/system/bluetooth.service.d";
    const DROPIN_CFG: &str = "/etc/systemd/system/bluetooth.service.d/livi-no-sap.conf";
    const MAIN_CONF: &str = "/etc/bluetooth/main.conf";

    fn find_bluetoothd() -> Option<String> {
        ["/usr/libexec/bluetooth/bluetoothd", "/usr/lib/bluetooth/bluetoothd", "/usr/sbin/bluetoothd"]
            .into_iter()
            .find(|p| Path::new(p).exists())
            .map(str::to_string)
    }

    fn write_noplugin_dropin() -> bool {
        let Some(bluetoothd) = find_bluetoothd() else {
            eprintln!("[helperd] bluetoothd binary not found, skipping --noplugin setup");
            return false;
        };
        let content =
            format!("[Service]\nExecStart=\nExecStart={bluetoothd} --noplugin={DISABLED_PLUGINS}\n");
        let mut changed = false;
        if let Ok(entries) = std::fs::read_dir(DROPIN_DIR) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("livi-") && name.ends_with(".conf") && entry.path() != Path::new(DROPIN_CFG)
                {
                    changed |= std::fs::remove_file(entry.path()).is_ok();
                }
            }
        }
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

    fn ensure_main_conf_class() -> bool {
        let Ok(original) = std::fs::read_to_string(MAIN_CONF) else {
            return false;
        };
        let new = with_class(&original);
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

    /// Applied before connecting to BlueZ; a restart here would drop the connection.
    pub fn setup() {
        let mut changed = write_noplugin_dropin();
        changed |= ensure_main_conf_class();
        if changed {
            println!("[helperd] restarting bluetoothd (--noplugin={DISABLED_PLUGINS}, class)");
            let _ = std::process::Command::new(crate::sys::tool("systemctl")).arg("daemon-reload").status();
            let _ = std::process::Command::new(crate::sys::tool("systemctl")).args(["restart", "bluetooth"]).status();
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
        assert_eq!(with_class(conf), "[General]\nName = x\n\nClass = 0x200418\n[Policy]\n");
    }

    #[test]
    fn appends_a_general_section_when_missing() {
        let conf = "[Policy]\nAutoEnable=true\n";
        assert_eq!(with_class(conf), "[Policy]\nAutoEnable=true\n\n[General]\nClass = 0x200418\n");
    }

    #[test]
    fn leaves_a_correct_conf_unchanged() {
        let conf = "[General]\nClass = 0x200418\n";
        assert_eq!(with_class(conf), conf);
    }
}
