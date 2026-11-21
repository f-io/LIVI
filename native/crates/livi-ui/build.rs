// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt::Write;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(winit)");
    println!("cargo::rustc-check-cfg=cfg(kms)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if env == "musl" {
        println!("cargo::rustc-cfg=kms");
    } else if os != "android" {
        println!("cargo::rustc-cfg=winit");
    }

    slint_build::compile("ui/app.slint").unwrap();

    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("ui/icons");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".svg").map(str::to_string))
        .collect();
    names.sort();
    let mut table = String::from("pub static ICONS: &[(&str, &[u8])] = &[\n");
    for n in &names {
        let path = dir.join(format!("{n}.svg"));
        writeln!(
            table,
            "    ({n:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    table.push_str("];\n");
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icons.rs");
    std::fs::write(out, table).unwrap();
}
