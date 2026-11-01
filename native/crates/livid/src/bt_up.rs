// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn run(args: Vec<String>) -> i32 {
    livid_main(&args)
}

use std::fs;
use std::io::Write;

use livi_btd::hci;

fn livid_main(args: &[String]) -> i32 {
    let dev: u16 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Err(e) = hci::up(dev) {
        eprintln!("livi-bt-up: HCIDEVUP hci{dev}: {e}");
        return 1;
    }
    let info = match hci::info(dev) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("livi-bt-up: HCIGETDEVINFO hci{dev}: {e}");
            return 2;
        }
    };

    let mac = format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        info.bdaddr[5],
        info.bdaddr[4],
        info.bdaddr[3],
        info.bdaddr[2],
        info.bdaddr[1],
        info.bdaddr[0]
    );
    let bus = info.dev_type & 0x0f;
    let kind = match (info.dev_type >> 4) & 0x03 {
        0 => "primary",
        1 => "amp",
        _ => "?",
    };
    println!("livi-bt-up: hci{dev} up, MAC {mac}, bus {bus}, type {kind}");

    let _ = fs::create_dir_all("/tmp/livi");
    if let Ok(mut f) = fs::File::create("/tmp/livi/bt-mac") {
        let _ = writeln!(f, "{mac}");
    }
    0
}
