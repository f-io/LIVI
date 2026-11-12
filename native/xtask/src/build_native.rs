// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::process::Command;

use crate::util::{Arch, build_date, build_run, build_sha, copy_exe, exec, read, root, version};

fn cargo(manifest: &PathBuf, sub: &[&str]) -> Result<Command, String> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.env("LIVI_VERSION", version()?)
        .env("LIVI_BUILD_DATE", build_date())
        .env("LIVI_BUILD_SHA", build_sha())
        .env("LIVI_BUILD_RUN", build_run())
        .env("PKG_CONFIG_ALLOW_CROSS", "1");
    cmd.args(sub).arg("--manifest-path").arg(manifest);
    Ok(cmd)
}

fn target_dir(manifest: &PathBuf) -> Result<PathBuf, String> {
    let meta = read(&mut cargo(
        manifest,
        &["metadata", "--format-version", "1", "--no-deps"],
    )?)?;
    let v: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
    v["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata gave no target_directory".into())
}

pub fn run(arch: Arch) -> Result<(), String> {
    let root = root();
    let manifest = root.join("native/Cargo.toml");
    let cross = arch != Arch::host();

    let mut pkgs = vec!["livi-output", "livi-helperd", "livi-core", "livi-ui"];
    if cfg!(target_os = "linux") {
        pkgs.push("livi-compositor");
    }

    let mut build = cargo(&manifest, &["build", "--release"])?;
    for p in &pkgs {
        build.args(["-p", p]);
    }
    if std::env::var_os("CI").is_some() {
        build.arg("--locked");
    }
    if cross {
        exec(Command::new("rustup").args(["target", "add", arch.triple()]))?;
        build.args(["--target", arch.triple()]);
    }
    exec(&mut build)?;

    let mut out = target_dir(&manifest)?;
    if cross {
        out.push(arch.triple());
    }
    out.push("release");

    let place = |name: &str, dest: PathBuf| -> Result<(), String> {
        copy_exe(&out.join(name), &dest)?;
        println!(
            "[build-native] {} <- {}",
            dest.display(),
            out.join(name).display()
        );
        Ok(())
    };
    let bin = root.join("out/bin");
    place("livi-output", bin.join("livi-output"))?;
    place("livi-helperd", bin.join("livi-helperd"))?;
    place("livi-core", bin.join("livi-core"))?;
    place("LIVI", bin.join("LIVI"))?;
    if cfg!(target_os = "linux") {
        place("livi-compositor", bin.join("livi-compositor"))?;
    }
    Ok(())
}
