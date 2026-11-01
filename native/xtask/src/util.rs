// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

impl Arch {
    pub fn host() -> Self {
        if std::env::consts::ARCH == "x86_64" {
            Self::X64
        } else {
            Self::Arm64
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "x64" => Some(Self::X64),
            "arm64" => Some(Self::Arm64),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
        }
    }

    pub fn appimage(self) -> &'static str {
        match self {
            Self::X64 => "x86_64",
            Self::Arm64 => "aarch64",
        }
    }

    pub fn triple(self) -> &'static str {
        match (self, cfg!(target_os = "macos")) {
            (Self::X64, true) => "x86_64-apple-darwin",
            (Self::Arm64, true) => "aarch64-apple-darwin",
            (Self::X64, false) => "x86_64-unknown-linux-gnu",
            (Self::Arm64, false) => "aarch64-unknown-linux-gnu",
        }
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask sits in native/xtask")
        .to_path_buf()
}

pub fn version() -> Result<String, String> {
    let toml = std::fs::read_to_string(root().join("native/Cargo.toml"))
        .map_err(|e| format!("native/Cargo.toml: {e}"))?;
    let section = toml
        .split("[workspace.package]")
        .nth(1)
        .ok_or("no [workspace.package] in native/Cargo.toml")?;
    section
        .lines()
        .take_while(|l| !l.starts_with('['))
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|v| v.strip_suffix('"'))
        .map(str::to_string)
        .ok_or_else(|| "no version in [workspace.package]".to_string())
}

pub fn build_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let (h, m) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02} UTC")
}

pub fn build_sha() -> String {
    let env = ["BUILD_SHA", "GITHUB_SHA"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()));
    let sha = env.or_else(|| {
        read(
            Command::new("git")
                .arg("-C")
                .arg(root())
                .args(["rev-parse", "HEAD"]),
        )
        .ok()
    });
    sha.map_or_else(|| "dev".into(), |s| s.trim().chars().take(7).collect())
}

pub fn build_run() -> String {
    ["GITHUB_RUN_NUMBER", "BUILD_RUN"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .unwrap_or_default()
}

pub fn exec(cmd: &mut Command) -> Result<(), String> {
    let what = format!("{cmd:?}");
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed with {status}"))
    }
}

pub fn fetch_gstreamer(platform: &str) -> Result<(), String> {
    exec(
        Command::new("sh")
            .arg(root().join("scripts/assets.sh"))
            .args(["fetch", "gstreamer", platform]),
    )
}

pub fn read(cmd: &mut Command) -> Result<String, String> {
    let what = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn copy_exe(src: &Path, dest: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fail = |e: std::io::Error| format!("{} -> {}: {e}", src.display(), dest.display());
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    std::fs::copy(src, dest).map_err(fail)?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)).map_err(fail)
}

pub fn copy_tree(src: &Path, dest: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        if skip(&from) {
            continue;
        }
        let to = dest.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            let link = std::fs::read_link(&from).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(link, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        } else if kind.is_dir() {
            copy_tree(&from, &to, skip)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_workspace() {
        let v = version().unwrap();
        assert!(v.split('.').count() == 3, "{v}");
    }

    #[test]
    fn the_build_date_is_utc_minutes() {
        let d = build_date();
        assert_eq!(d.len(), "2026-10-20 14:30 UTC".len());
        assert!(d.ends_with(" UTC"));
    }
}
