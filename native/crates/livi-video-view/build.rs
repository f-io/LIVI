// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const GST_PKGS: [&str; 3] = ["gstreamer-1.0", "gstreamer-app-1.0", "gstreamer-video-1.0"];

fn gst_includes() -> Vec<std::path::PathBuf> {
    let mut includes = Vec::new();
    for pkg in GST_PKGS {
        let lib = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(pkg)
            .unwrap();
        includes.extend(lib.include_paths);
    }
    includes
}

fn emit_gst_link_flags() {
    let first = pkg_config::Config::new().probe(GST_PKGS[0]).unwrap();
    let mut seen: std::collections::HashSet<std::path::PathBuf> =
        first.link_paths.iter().cloned().collect();

    for pkg in &GST_PKGS[1..] {
        let lib = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(pkg)
            .unwrap();
        for path in &lib.link_paths {
            if seen.insert(path.clone()) {
                println!("cargo:rustc-link-search=native={}", path.display());
            }
        }
        for name in &lib.libs {
            println!("cargo:rustc-link-lib={name}");
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/gst_video_mac.mm");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag("-mmacosx-version-min=11.0")
        .includes(gst_includes())
        .link_lib_modifier("+whole-archive")
        .file("src/gst_video_mac.mm")
        .compile("gst_video_cpp");
    println!("cargo:rustc-link-lib=framework=Cocoa");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    println!("cargo:rustc-link-lib=framework=CoreImage");

    emit_gst_link_flags();
}
