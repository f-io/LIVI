// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{Arch, copy_exe, copy_tree, exec, fetch_gstreamer, read, root, version};

const APPIMAGETOOL: &str = "https://api.github.com/repos/AppImage/appimagetool/releases/latest";

const START: &str = r#"#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
[ "$1" = telemetry ] && exec "$HERE/resources/core/livi-core" "$@"
export LIVI_RESOURCES="$HERE/resources"
exec "$HERE/resources/core/livi-core"
"#;

const DESKTOP: &str = "[Desktop Entry]
Name=LIVI
Comment=Linux In-Vehicle Infotainment
Exec=AppRun %U
Icon=livi
Type=Application
Terminal=false
Categories=Utility;
StartupWMClass=LIVI
";

const TEMPLATES: [&str; 6] = [
    "99-LIVI.rules.template",
    "99-LIVI-helper.sudoers.template",
    "livi-wifi-ap.service.template",
    "99-LIVI-wifi-ap.sudoers.template",
    "livi-kiosk.service.template",
    "livi-touch-filter",
];

fn copy(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::copy(src, dest)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", src.display(), dest.display()))
}

fn write_exe(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

fn sha256(path: &Path) -> Result<String, String> {
    let sum = read(Command::new("sha256sum").arg(path))?;
    Ok(sum
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string())
}

fn cached(dir: &Path, prefix: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && !n.ends_with(".part"))
        })
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

fn appimagetool() -> Result<PathBuf, String> {
    if let Ok(tool) = std::env::var("APPIMAGETOOL") {
        return Ok(tool.into());
    }
    let host = std::env::consts::ARCH;
    let asset = format!("appimagetool-{host}.AppImage");
    let prefix = format!("appimagetool-{host}-");
    let dir = root().join("native/target/tools");
    let release = read(Command::new("curl").args(["-fsSL", "--retry", "3", APPIMAGETOOL]));
    let release: serde_json::Value = match release {
        Ok(json) => serde_json::from_str(&json).map_err(|e| format!("{APPIMAGETOOL}: {e}"))?,
        Err(e) => {
            return cached(&dir, &prefix).ok_or(e).inspect(|tool| {
                println!("[package-linux] offline, using {}", tool.display());
            });
        }
    };
    let found = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == asset.as_str())
        .ok_or_else(|| format!("no {asset} in the latest appimagetool release"))?;
    let url = found["browser_download_url"]
        .as_str()
        .ok_or_else(|| format!("{asset} has no download url"))?;
    let want = found["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .ok_or_else(|| format!("{asset} has no sha256"))?;
    let name = format!("{prefix}{}", &want[..want.len().min(12)]);
    let tool = dir.join(&name);
    if tool.exists() && sha256(&tool)? == want {
        return Ok(tool);
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let part = dir.join(format!("{name}.part"));
    println!("[package-linux] fetching {url}");
    exec(
        Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&part)
            .arg(url),
    )?;
    let got = sha256(&part)?;
    if got != want {
        let _ = fs::remove_file(&part);
        return Err(format!("{asset}: sha256 {got}, expected {want}"));
    }
    fs::set_permissions(&part, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    fs::rename(&part, &tool).map_err(|e| e.to_string())?;
    for old in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let old = old.path();
        let stale = old
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&prefix));
        if stale && old != tool {
            let _ = fs::remove_file(&old);
        }
    }
    Ok(tool)
}

pub fn run(arch: Arch) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("package-linux runs on Linux".into());
    }
    let tool = appimagetool()?;
    crate::build_native::run(arch)?;

    let root = root();
    let version = version()?;
    let dist = root.join("dist");
    let app = dist.join(format!("linux-{}-unpacked", arch.name()));
    let res = app.join("resources");
    let _ = fs::remove_dir_all(&app);
    fs::create_dir_all(&res).map_err(|e| e.to_string())?;

    let built = |name: &str| root.join("out/bin").join(name);
    copy_exe(&built("LIVI"), &app.join("livi-ui"))?;
    copy_exe(&built("livi-core"), &res.join("core/livi-core"))?;
    copy_exe(&built("livi-helperd"), &res.join("driver/livi-helperd"))?;
    copy_exe(&built("livi-gst-host"), &res.join("gst-host/livi-gst-host"))?;
    copy_exe(
        &built("livi-compositor"),
        &res.join("compositor/livi-compositor"),
    )?;

    let linux = root.join("assets/linux");
    for name in TEMPLATES {
        copy(&linux.join(name), &res.join(name))?;
    }
    for legal in ["LICENSE", "NOTICE"] {
        copy(&root.join(legal), &res.join(legal))?;
    }
    fs::set_permissions(
        res.join("livi-touch-filter"),
        fs::Permissions::from_mode(0o755),
    )
    .map_err(|e| e.to_string())?;
    copy(
        &linux.join("dev.f-io.livi.metainfo.xml"),
        &app.join("share/metainfo/dev.f-io.livi.metainfo.xml"),
    )?;
    copy_exe(
        &root.join("scripts/install/pi/setup-hdmi-pr-display.sh"),
        &res.join("setup-hdmi-pr-display.sh"),
    )?;
    let displays = root.join("assets/displays");
    if displays.is_dir() {
        copy_tree(&displays, &res.join("displays"), &|_| false)?;
    }

    fetch_gstreamer(&format!("linux-{}", arch.name()))?;
    let gst_dir = format!("gstreamer/linux-{}", arch.name());
    let gst_src = root.join("assets").join(&gst_dir);
    let skip = |p: &Path| {
        p.extension().is_some_and(|e| e == "a")
            || p.strip_prefix(&gst_src)
                .is_ok_and(|r| r.starts_with("share"))
    };
    copy_tree(&gst_src, &res.join(&gst_dir), &skip)?;

    write_exe(&app.join("LIVI"), START)?;
    std::os::unix::fs::symlink("LIVI", app.join("AppRun")).map_err(|e| e.to_string())?;
    fs::write(app.join("dev.f-io.livi.desktop"), DESKTOP).map_err(|e| e.to_string())?;
    copy(
        &root.join("assets/icons/linux/livi.png"),
        &app.join("livi.png"),
    )?;

    let image = dist.join(format!("LIVI-{version}-linux-{}.AppImage", arch.name()));
    let _ = fs::remove_file(&image);
    exec(
        Command::new(&tool)
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .env("ARCH", arch.appimage())
            .env("VERSION", &version)
            .arg("--no-appstream")
            .arg(&app)
            .arg(&image),
    )?;
    println!("[package-linux] {}", image.display());
    Ok(())
}
