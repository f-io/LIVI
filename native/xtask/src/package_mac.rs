// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{Arch, copy_exe, copy_tree, exec, fetch_gstreamer, read, root, version};

const BUNDLE_ID: &str = "dev.f-io.livi";
const GST: &str = "gstreamer/macos-arm64";

fn plist(entries: &[(&str, &str)]) -> String {
    let body: Vec<String> = entries
        .iter()
        .map(|(k, v)| {
            let value = match *v {
                "true" => "<true/>".to_string(),
                "false" => "<false/>".to_string(),
                s => format!("<string>{s}</string>"),
            };
            format!("  <key>{k}</key>\n  {value}")
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{}\n</dict>\n</plist>\n",
        body.join("\n")
    )
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            files_under(&entry.path(), out)?;
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

fn relink(file: &Path, rpath: &str) -> Result<(), String> {
    let load = read(Command::new("otool").arg("-l").arg(file))?;
    let lines: Vec<&str> = load.lines().map(str::trim).collect();
    let mut cmd = Command::new("install_name_tool");
    for (i, l) in lines.iter().enumerate() {
        if *l == "cmd LC_RPATH"
            && let Some(path) = lines
                .get(i + 2)
                .and_then(|p| p.strip_prefix("path "))
                .and_then(|p| p.split(" (offset").next())
        {
            cmd.args(["-delete_rpath", path]);
        }
    }
    exec(cmd.args(["-add_rpath", rpath]).arg(file))
}

fn sign(file: &Path, entitlements: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new("codesign");
    cmd.args([
        "--force",
        "--sign",
        "-",
        "--timestamp=none",
        "--options",
        "runtime",
    ]);
    if let Some(e) = entitlements {
        cmd.arg("--entitlements").arg(e);
    }
    exec(cmd.arg(file))
}

fn mach_o(file: &Path) -> bool {
    read(Command::new("file").arg("-b").arg(file)).is_ok_and(|t| t.starts_with("Mach-O"))
}

fn has_diskutil_images() -> bool {
    Command::new("diskutil")
        .args(["image", "create", "from", "--help"])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn run(arch: Arch) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("package-mac runs on macOS".into());
    }
    crate::build_native::run(arch)?;

    let root = root();
    let version = version()?;
    let dist = root.join("dist");
    let app = dist.join("LIVI.app");
    let contents = app.join("Contents");
    let res = contents.join("Resources");
    let _ = fs::remove_dir_all(&app);
    fs::create_dir_all(contents.join("MacOS")).map_err(|e| e.to_string())?;

    let built = |name: &str| root.join("out/bin").join(name);
    copy_exe(&built("LIVI"), &contents.join("MacOS/LIVI"))?;
    copy_exe(&built("livi-core"), &res.join("core/livi-core"))?;
    copy_exe(&built("livi-helperd"), &res.join("driver/livi-helperd"))?;
    copy_exe(&built("livi-gst-host"), &res.join("gst-host/livi-gst-host"))?;

    fetch_gstreamer("macos-arm64")?;
    let gst_src = root.join("assets").join(GST);
    let skip = |p: &Path| {
        p.extension().is_some_and(|e| e == "a")
            || p.strip_prefix(&gst_src)
                .is_ok_and(|r| r.starts_with("share"))
    };
    copy_tree(&gst_src, &res.join(GST), &skip)?;
    fs::copy(
        root.join("assets/icons/mac/livi.icns"),
        res.join("livi.icns"),
    )
    .map_err(|e| e.to_string())?;
    for legal in ["LICENSE", "NOTICE"] {
        fs::copy(root.join(legal), res.join(legal)).map_err(|e| e.to_string())?;
    }

    fs::write(
        contents.join("Info.plist"),
        plist(&[
            ("CFBundleIdentifier", BUNDLE_ID),
            ("CFBundleName", "LIVI"),
            ("CFBundleDisplayName", "LIVI"),
            ("CFBundleExecutable", "LIVI"),
            ("CFBundleIconFile", "livi.icns"),
            ("CFBundlePackageType", "APPL"),
            ("CFBundleShortVersionString", &version),
            ("CFBundleVersion", &version),
            ("CFBundleInfoDictionaryVersion", "6.0"),
            ("LSMinimumSystemVersion", "11.0"),
            ("LSApplicationCategoryType", "public.app-category.utilities"),
            ("NSHighResolutionCapable", "true"),
            (
                "NSHumanReadableCopyright",
                "Copyright © 2026 Lasse Heitgres",
            ),
            (
                "NSCameraUsageDescription",
                "LIVI requires camera access for CarPlay.",
            ),
            (
                "NSMicrophoneUsageDescription",
                "LIVI requires microphone access for voice control.",
            ),
            (
                "NSLocalNetworkUsageDescription",
                "LIVI talks to a LIVI Link dongle on the local network.",
            ),
        ]),
    )
    .map_err(|e| e.to_string())?;

    relink(
        &contents.join("MacOS/LIVI"),
        &format!("@executable_path/../Resources/{GST}/lib"),
    )?;
    relink(
        &res.join("gst-host/livi-gst-host"),
        &format!("@loader_path/../{GST}/lib"),
    )?;

    let inherit = root.join("assets/mac/entitlements.mac.inherit.plist");
    let main = root.join("assets/mac/entitlements.mac.plist");
    let mut nested = Vec::new();
    files_under(&res, &mut nested)?;
    let nested: Vec<PathBuf> = nested.into_iter().filter(|f| mach_o(f)).collect();
    let executable = |f: &PathBuf| {
        let exec = fs::metadata(f).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
        exec && !f.extension().is_some_and(|e| e == "dylib" || e == "so")
    };
    for f in nested.iter().filter(|f| !executable(f)) {
        sign(f, None)?;
    }
    for f in nested.iter().filter(|f| executable(f)) {
        sign(f, Some(&inherit))?;
    }
    sign(&app, Some(&main))?;
    exec(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict", "--verbose=2"])
            .arg(&app),
    )?;

    let staging = dist.join("dmg");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    exec(
        Command::new("ditto")
            .arg(&app)
            .arg(staging.join("LIVI.app")),
    )?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))
        .map_err(|e| e.to_string())?;
    let dmg = dist.join(format!("LIVI-{version}-macos-{}.dmg", arch.name()));
    let _ = fs::remove_file(&dmg);
    if has_diskutil_images() {
        exec(
            Command::new("diskutil")
                .args([
                    "image",
                    "create",
                    "from",
                    "--format",
                    "UDZO",
                    "--volumeName",
                    "LIVI",
                ])
                .arg(&staging)
                .arg(&dmg),
        )?;
    } else {
        exec(
            Command::new("hdiutil")
                .args([
                    "create",
                    "-volname",
                    "LIVI",
                    "-format",
                    "UDZO",
                    "-srcfolder",
                ])
                .arg(&staging)
                .arg(&dmg),
        )?;
    }
    let _ = fs::remove_dir_all(&staging);
    println!("[package-mac] {}", dmg.display());
    Ok(())
}
