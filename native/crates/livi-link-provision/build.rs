// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

fn main() {
    let embedded = [
        "../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb",
        "../../../assets/livi-link/ax520_aic8800d80/livi-link-ax520.lfwb",
        "../../../assets/livi-link/imx6ul_iw416/livi-link-imx6ull.lfwb",
        "../../../assets/livi-link/imx6ul_rtl8822cs/livi-link-imx6ull-rtl8822cs.lfwb",
        "../../../assets/livi-link/imx6ul_rtl8822bs/livi-link-imx6ull-rtl8822bs.lfwb",
        "../../../assets/livi-link/imx6ul_bcm4354/livi-link-imx6ull-bcm4354.lfwb",
        "assets/bindshell/armv7",
        "assets/bindshell/riscv32",
        "assets/bindshell/mipsel",
    ];
    for file in embedded {
        let path = Path::new(file);
        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(path, b"");
            println!(
                "cargo:warning=empty {} stub — CI populates it when its target is built",
                path.file_name().unwrap().to_string_lossy()
            );
        }
        println!("cargo:rerun-if-changed={file}");
    }
}
