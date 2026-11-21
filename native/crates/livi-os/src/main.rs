// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The name it is started under picks the part. Any other name, core restarting itself included, is core.

use std::process::ExitCode;

#[cfg(target_env = "musl")]
fn main() -> ExitCode {
    let arg0 = std::env::args_os().next().unwrap_or_default();
    let name = std::path::Path::new(&arg0).file_name().unwrap_or_default();
    match name.to_str() {
        Some("LIVI") => match livi_ui::main() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e:?}");
                ExitCode::FAILURE
            }
        },
        Some("livi-helperd") => livi_helperd::main(),
        Some(
            "livid" | "livi-bt-up" | "livi-btd" | "livi-httpd" | "livi-ledd" | "livi-mdnsd"
            | "livi-mfid" | "livi-netd" | "livi-wifid",
        ) => livid::main(),
        Some("livi-output") => {
            livi_output::main();
            ExitCode::SUCCESS
        }
        _ => livi_core::main(),
    }
}

#[cfg(not(target_env = "musl"))]
fn main() -> ExitCode {
    eprintln!(
        "livi is the LIVI OS build, other systems run LIVI, livi-core, livi-helperd and livi-output"
    );
    ExitCode::FAILURE
}
