// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::util::{
    build_date, build_run, build_sha, copy_tree, exec, fetch_gstreamer, root, version,
};

const TARGET: &str = "aarch64-apple-ios";
const GST: &str = "ios-arm64";
const MIN_IOS: &str = "15.0";
const EXE: &str = "livi";
const ICONS: [(&str, u32); 3] = [
    ("AppIcon76x76~ipad.png", 76),
    ("AppIcon76x76@2x~ipad.png", 152),
    ("AppIcon83.5x83.5@2x~ipad.png", 167),
];

fn cargo() -> Result<Command, String> {
    fetch_gstreamer(GST)?;
    let pkgconfig = root()
        .join("assets/gstreamer")
        .join(GST)
        .join("lib/pkgconfig");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root().join("native"))
        .env("LIVI_VERSION", version()?)
        .env("LIVI_BUILD_DATE", build_date())
        .env("LIVI_BUILD_SHA", build_sha())
        .env("LIVI_BUILD_RUN", build_run())
        .env("IPHONEOS_DEPLOYMENT_TARGET", MIN_IOS)
        .env("PKG_CONFIG_ALLOW_CROSS", "1")
        .env("PKG_CONFIG_PATH_aarch64_apple_ios", &pkgconfig)
        .env("PKG_CONFIG_LIBDIR_aarch64_apple_ios", &pkgconfig);
    Ok(cmd)
}

pub fn check() -> Result<(), String> {
    exec(cargo()?.args([
        "clippy", "--locked", "--target", TARGET, "-p", "livi-ios", "--", "-D", "warnings",
    ]))
}

pub fn run() -> Result<(), String> {
    let root = root();
    let version = version()?;
    exec(cargo()?.args([
        "build",
        "--release",
        "--locked",
        "--target",
        TARGET,
        "-p",
        "livi-ios",
    ]))?;

    let build = root.join("ios/build");
    let _ = fs::remove_dir_all(&build);
    let app = build.join("Payload/LIVI.app");
    let frameworks = app.join("Frameworks");
    fs::create_dir_all(&frameworks).map_err(|e| e.to_string())?;

    let exe = root
        .join("native/target")
        .join(TARGET)
        .join("release")
        .join(EXE);
    crate::util::copy_exe(&exe, &app.join(EXE))?;
    copy_tree(
        &root
            .join("assets/gstreamer")
            .join(GST)
            .join("lib/GStreamer.framework"),
        &frameworks.join("GStreamer.framework"),
        &|_| false,
    )?;

    let plist = app.join("Info.plist");
    fs::copy(root.join("ios/Info.plist"), &plist).map_err(|e| format!("Info.plist: {e}"))?;
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        exec(
            Command::new("plutil")
                .args(["-replace", key, "-string", &version])
                .arg(&plist),
        )?;
    }
    for (name, px) in ICONS {
        exec(
            Command::new("sips")
                .args(["-z", &px.to_string(), &px.to_string()])
                .arg(root.join("assets/icons/livi.png"))
                .arg("--out")
                .arg(app.join(name))
                .stdout(std::process::Stdio::null()),
        )?;
    }

    sign(&app, &build)?;

    let dist = root.join("dist");
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let ipa = dist.join(format!("LIVI-{version}-ios-arm64.ipa"));
    let _ = fs::remove_file(&ipa);
    exec(
        Command::new("ditto")
            .current_dir(&build)
            .args(["-c", "-k", "--keepParent", "Payload"])
            .arg(&ipa),
    )?;
    println!("[package-ios] {}", ipa.display());
    Ok(())
}

fn sign(app: &Path, build: &Path) -> Result<(), String> {
    let (Some(identity), Some(profile)) = (
        std::env::var_os("LIVI_IOS_IDENTITY"),
        std::env::var_os("LIVI_IOS_PROFILE"),
    ) else {
        println!("[package-ios] no LIVI_IOS_IDENTITY and LIVI_IOS_PROFILE, the app stays unsigned");
        return Ok(());
    };
    fs::copy(&profile, app.join("embedded.mobileprovision"))
        .map_err(|e| format!("{}: {e}", Path::new(&profile).display()))?;
    let decoded = build.join("profile.plist");
    exec(
        Command::new("security")
            .args(["cms", "-D", "-i"])
            .arg(&profile)
            .arg("-o")
            .arg(&decoded),
    )?;
    let entitlements = build.join("entitlements.plist");
    exec(
        Command::new("plutil")
            .args(["-extract", "Entitlements", "xml1", "-o"])
            .arg(&entitlements)
            .arg(&decoded),
    )?;
    let codesign = |path: &Path, entitlements: Option<&Path>| {
        let mut cmd = Command::new("codesign");
        cmd.args(["--force", "--timestamp=none", "--sign"])
            .arg(&identity);
        if let Some(e) = entitlements {
            cmd.arg("--entitlements").arg(e);
        }
        exec(cmd.arg(path))
    };
    codesign(&app.join("Frameworks/GStreamer.framework"), None)?;
    codesign(app, Some(&entitlements))
}
