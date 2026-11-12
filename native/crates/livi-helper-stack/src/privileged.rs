// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum PrivilegedError {
    #[error("needs root")]
    NeedsRoot,
    #[error("{path}: {source}")]
    File { path: String, source: io::Error },
    #[error("the rule did not pass visudo")]
    Visudo,
    #[error("{0} is not installed")]
    NotInstalled(&'static str),
    #[error("own binary: {0}")]
    OwnBinary(io::Error),
}

fn at(path: impl std::fmt::Display) -> impl FnOnce(io::Error) -> PrivilegedError {
    move |source| PrivilegedError::File {
        path: path.to_string(),
        source,
    }
}

fn require_root() -> Result<(), PrivilegedError> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err(PrivilegedError::NeedsRoot)
    }
}

fn read(src: &str) -> Result<Vec<u8>, PrivilegedError> {
    std::fs::read(src).map_err(at(src))
}

pub fn write_root_file(path: &str, content: &[u8], mode: u32) -> Result<(), PrivilegedError> {
    require_root()?;
    livi_base::fs::write_atomic(std::path::Path::new(path), content, mode).map_err(at(path))
}

pub fn install_sudoers(file: &str, content: &[u8]) -> Result<(), PrivilegedError> {
    let staged = format!("{file}.livi-tmp");
    write_root_file(&staged, content, 0o440)?;
    let checked = Command::new(livi_base::sys::tool("visudo"))
        .args(["-c", "-f", &staged])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !checked {
        let _ = std::fs::remove_file(&staged);
        return Err(PrivilegedError::Visudo);
    }
    std::fs::rename(&staged, file).map_err(at(file))
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(livi_base::sys::tool(cmd))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

const UDEV_RULE: &str = "/etc/udev/rules.d/99-LIVI.rules";
const TOUCH_FILTER: &str = "/usr/local/lib/livi/livi-touch-filter";

pub fn install_udev_rule(rule_src: &str, filter_src: Option<&str>) -> Result<(), PrivilegedError> {
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

pub fn install_gvfs_guard(script_src: &str, rule_src: &str) -> Result<(), PrivilegedError> {
    write_root_file(GVFS_GUARD, &read(script_src)?, 0o755)?;
    install_sudoers(GVFS_SUDOERS, &read(rule_src)?)
}

const AP_UNIT: &str = "/etc/systemd/system/livi-wifi-ap.service";
const AP_SUDOERS: &str = "/etc/sudoers.d/99-LIVI-wifi-ap";
const AP_HELPER: &str = "/usr/local/sbin/livi-helperd";

pub fn install_wifi_ap(unit_src: &str, rule_src: &str) -> Result<(), PrivilegedError> {
    install_sudoers(AP_SUDOERS, &read(rule_src)?)?;
    install_own_copy(AP_HELPER)?;
    write_root_file(AP_UNIT, &read(unit_src)?, 0o644)?;
    run("systemctl", &["daemon-reload"]);
    Ok(())
}

const KIOSK_UNIT: &str = "/etc/systemd/system/livi-kiosk.service";

pub fn install_kiosk_unit(unit_src: &str) -> Result<(), PrivilegedError> {
    if !std::path::Path::new(KIOSK_UNIT).exists() {
        return Err(PrivilegedError::NotInstalled(KIOSK_UNIT));
    }
    write_root_file(KIOSK_UNIT, &read(unit_src)?, 0o644)?;
    run("systemctl", &["daemon-reload"]);
    let systemctl = livi_base::sys::tool("systemctl");
    let switch =
        format!("{systemctl} stop livi-kiosk.service; {systemctl} start livi-kiosk.service");
    run(
        "systemd-run",
        &[
            "--no-block",
            "--collect",
            "--unit=livi-kiosk-switch",
            "/bin/sh",
            "-c",
            &switch,
        ],
    );
    Ok(())
}

fn install_own_copy(path: &str) -> Result<(), PrivilegedError> {
    require_root()?;
    let me = std::env::current_exe().map_err(PrivilegedError::OwnBinary)?;
    let staged = format!("{path}.livi-tmp");
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(at(dir.display()))?;
    }
    std::fs::copy(&me, &staged).map_err(at(&staged))?;
    std::fs::set_permissions(&staged, PermissionsExt::from_mode(0o755)).map_err(at(&staged))?;
    std::fs::rename(&staged, path).map_err(at(path))
}
