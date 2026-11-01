// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod build_native;
mod package_linux;
mod package_mac;
mod util;

use std::process::ExitCode;

use util::Arch;

const USAGE: &str =
    "usage: cargo xtask <build-native|package-mac|package-linux> [--arch x64|arm64]";

fn arch(args: &[String]) -> Result<Arch, String> {
    let mut it = args.iter();
    let mut arch = Arch::host();
    while let Some(a) = it.next() {
        let value = match a.strip_prefix("--arch=") {
            Some(v) => v.to_string(),
            None if a == "--arch" => it.next().cloned().ok_or(USAGE)?,
            None => return Err(format!("unknown argument {a}\n{USAGE}")),
        };
        arch = Arch::parse(&value).ok_or(USAGE)?;
    }
    Ok(arch)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("build-native") => arch(&args[1..]).and_then(build_native::run),
        Some("package-mac") => arch(&args[1..]).and_then(package_mac::run),
        Some("package-linux") => arch(&args[1..]).and_then(package_linux::run),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[xtask] {e}");
            ExitCode::FAILURE
        }
    }
}
