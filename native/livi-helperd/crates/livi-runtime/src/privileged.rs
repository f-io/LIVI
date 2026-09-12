//! Files the app cannot write itself, put in place by this helper running as root. The app
//! renders each file and hands its path over, so what lands is what the app would have shown.

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

fn require_root() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err("needs root".into())
    }
}

fn read(src: &str) -> Result<Vec<u8>, String> {
    std::fs::read(src).map_err(|e| format!("{src}: {e}"))
}

/// Writes `content` to `path` with `mode`, creating the directory above it.
pub fn write_root_file(path: &str, content: &[u8], mode: u32) -> Result<(), String> {
    require_root()?;
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("{path}: {e}"))?;
    std::fs::set_permissions(path, PermissionsExt::from_mode(mode))
        .map_err(|e| format!("{path}: {e}"))
}

/// A sudoers drop-in lands only after visudo accepted it.
pub fn install_sudoers(file: &str, content: &[u8]) -> Result<(), String> {
    let staged = format!("{file}.livi-tmp");
    write_root_file(&staged, content, 0o440)?;
    let checked = Command::new(crate::sys::tool("visudo"))
        .args(["-c", "-f", &staged])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !checked {
        let _ = std::fs::remove_file(&staged);
        return Err("the rule did not pass visudo".into());
    }
    std::fs::rename(&staged, file).map_err(|e| format!("{file}: {e}"))
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(crate::sys::tool(cmd))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

const UDEV_RULE: &str = "/etc/udev/rules.d/99-LIVI.rules";
const TOUCH_FILTER: &str = "/usr/local/lib/livi/livi-touch-filter";

/// `--install-udev-rule <rule> [<touch filter>]`
pub fn install_udev_rule(rule_src: &str, filter_src: Option<&str>) -> Result<(), String> {
    write_root_file(UDEV_RULE, &read(rule_src)?, 0o644)?;
    if let Some(filter) = filter_src {
        write_root_file(TOUCH_FILTER, &read(filter)?, 0o755)?;
    }
    run("udevadm", &["control", "--reload-rules"]);
    run("udevadm", &["trigger"]);
    Ok(())
}

const GVFS_GUARD: &str = "/usr/local/lib/livi/gvfs-phone-guard.sh";
const GVFS_SUDOERS: &str = "/etc/sudoers.d/99-LIVI-gvfs";

/// `--install-gvfs-guard <script> <rule>`
pub fn install_gvfs_guard(script_src: &str, rule_src: &str) -> Result<(), String> {
    write_root_file(GVFS_GUARD, &read(script_src)?, 0o755)?;
    install_sudoers(GVFS_SUDOERS, &read(rule_src)?)
}

const AP_UNIT: &str = "/etc/systemd/system/livi-wifi-ap.service";
const AP_SUDOERS: &str = "/etc/sudoers.d/99-LIVI-wifi-ap";

/// `--install-wifi-ap <unit> <rule>`
pub fn install_wifi_ap(unit_src: &str, rule_src: &str) -> Result<(), String> {
    install_sudoers(AP_SUDOERS, &read(rule_src)?)?;
    write_root_file(AP_UNIT, &read(unit_src)?, 0o644)?;
    run("systemctl", &["daemon-reload"]);
    Ok(())
}
