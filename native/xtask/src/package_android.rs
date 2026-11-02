// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{
    build_date, build_run, build_sha, copy_tree, exec, fetch_gstreamer, root, version,
};

const ABI: &str = "arm64-v8a";
const TARGET: &str = "aarch64_linux_android";
const GST: &str = "android-arm64";

fn sdk() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("ANDROID_HOME") {
        return Ok(dir.into());
    }
    let home = std::env::var_os("HOME").ok_or("no HOME")?;
    let dir = Path::new(&home).join(if cfg!(target_os = "macos") {
        "Library/Android/sdk"
    } else {
        "Android/Sdk"
    });
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(format!(
            "no Android SDK in {}, set ANDROID_HOME",
            dir.display()
        ))
    }
}

fn numbers(version: &str) -> Vec<u32> {
    version.split('.').map_while(|p| p.parse().ok()).collect()
}

fn newest(dir: &Path, version_of: impl Fn(&str) -> Option<&str>) -> Result<PathBuf, String> {
    fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| Some((numbers(version_of(&name)?), name)))
        .max()
        .map(|(_, name)| dir.join(name))
        .ok_or_else(|| format!("nothing usable in {}", dir.display()))
}

fn ndk(sdk: &Path, android: &Path) -> Result<PathBuf, String> {
    match std::env::var_os("ANDROID_NDK_HOME") {
        Some(dir) => Ok(dir.into()),
        None => Ok(sdk.join("ndk").join(property(android, "livi.ndk")?)),
    }
}

fn property(android: &Path, key: &str) -> Result<String, String> {
    let props = android.join("gradle.properties");
    fs::read_to_string(&props)
        .map_err(|e| format!("{}: {e}", props.display()))?
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim().to_string())
        .ok_or_else(|| format!("no {key} in {}", props.display()))
}

fn android_jar(sdk: &Path, compile_sdk: &str) -> Result<PathBuf, String> {
    let platform = newest(&sdk.join("platforms"), |name| {
        let version = name.strip_prefix("android-")?;
        (version == compile_sdk || version.starts_with(&format!("{compile_sdk}.")))
            .then_some(version)
    })?;
    Ok(platform.join("android.jar"))
}

fn cargo(sdk: &Path, android: &Path) -> Result<Command, String> {
    fetch_gstreamer(GST)?;
    let pkgconfig = root()
        .join("assets/gstreamer")
        .join(GST)
        .join("lib/pkgconfig");
    let compile_sdk = property(android, "livi.compileSdk")?;
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root().join("native"))
        .env("LIVI_VERSION", version()?)
        .env("LIVI_BUILD_DATE", build_date())
        .env("LIVI_BUILD_SHA", build_sha())
        .env("LIVI_BUILD_RUN", build_run())
        .env("PKG_CONFIG_ALLOW_CROSS", "1")
        .env(format!("PKG_CONFIG_PATH_{TARGET}"), &pkgconfig)
        .env(format!("PKG_CONFIG_LIBDIR_{TARGET}"), &pkgconfig)
        .env("ANDROID_HOME", sdk)
        .env("ANDROID_NDK_HOME", ndk(sdk, android)?)
        .env("ANDROID_JAR", android_jar(sdk, &compile_sdk)?)
        .args(["ndk", "-t", ABI, "-P", &property(android, "livi.minSdk")?]);
    Ok(cmd)
}

pub fn check() -> Result<(), String> {
    let android = root().join("android");
    exec(cargo(&sdk()?, &android)?.args([
        "clippy",
        "--locked",
        "-p",
        "livi-android",
        "--",
        "-D",
        "warnings",
        "-A",
        "clippy::missing_const_for_thread_local",
    ]))
}

pub fn run() -> Result<(), String> {
    let root = root();
    let version = version()?;
    let sdk = sdk()?;
    let android = root.join("android");
    let generated = android.join("app/build/generated/livi");
    let _ = fs::remove_dir_all(&generated);

    exec(
        cargo(&sdk, &android)?
            .arg("-o")
            .arg(generated.join("jniLibs"))
            .args(["build", "--release", "--locked", "-p", "livi-android"]),
    )?;

    let gst = root.join("assets/gstreamer").join(GST);
    let libs = generated.join("jniLibs").join(ABI);
    fs::create_dir_all(&libs).map_err(|e| e.to_string())?;
    let gst_lib = "libgstreamer_android.so";
    fs::copy(gst.join("lib").join(gst_lib), libs.join(gst_lib))
        .map_err(|e| format!("{gst_lib}: {e}"))?;
    copy_tree(&gst.join("java"), &generated.join("java"), &|_| false)?;
    copy_tree(&gst.join("assets"), &generated.join("assets"), &|_| false)?;

    let icon = generated.join("res/mipmap-xxxhdpi/ic_launcher.png");
    fs::create_dir_all(icon.parent().expect("the icon sits in a directory"))
        .map_err(|e| e.to_string())?;
    fs::copy(root.join("assets/icons/linux/livi.png"), &icon)
        .map_err(|e| format!("{}: {e}", icon.display()))?;

    if std::env::var_os("LIVI_ANDROID_KEYSTORE").is_none() {
        println!("[package-android] no LIVI_ANDROID_KEYSTORE, signing with the debug key");
    }
    exec(
        Command::new(android.join("gradlew"))
            .current_dir(&android)
            .env("ANDROID_HOME", &sdk)
            .arg(":app:assembleRelease"),
    )?;

    let dist = root.join("dist");
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let apk = dist.join(format!("LIVI-{version}-android-arm64.apk"));
    fs::copy(
        android.join("app/build/outputs/apk/release/app-release.apk"),
        &apk,
    )
    .map_err(|e| format!("{}: {e}", apk.display()))?;
    println!("[package-android] {}", apk.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(numbers("30.0.16248370") > numbers("9.1.0"));
        assert!(numbers("37.2") > numbers("37"));
    }
}
