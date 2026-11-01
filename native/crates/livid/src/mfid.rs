// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::net::TcpListener;

use iap2_mfi::I2cCoprocessor;
use iap2_mfi::server::{listen, protocol_major};
use livi_net::port::MFI;

pub const STATE: &str = "/tmp/livi/mfi";

pub fn run(args: Vec<String>) -> i32 {
    let bus: u32 = args
        .first()
        .map(|a| a.trim_start_matches("/dev/i2c-"))
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(1);
    let mut chip = match I2cCoprocessor::open(bus, -1) {
        Ok(chip) => chip,
        Err(e) => {
            eprintln!("[mfid] no MFi chip on i2c-{bus}: {e}");
            let _ = fs::remove_file(STATE);
            return 1;
        }
    };
    let addr = chip.address();
    let major = protocol_major(&mut chip).map(|m| m.to_string());
    println!(
        "[mfid] MFi @0x{addr:02X} on i2c-{bus}, protocol major {}",
        major.as_deref().unwrap_or("unknown")
    );
    let _ = fs::write(
        STATE,
        format!("Gen{} @ /dev/i2c-{bus}\n", major.as_deref().unwrap_or("?")),
    );

    let listener = match TcpListener::bind(("0.0.0.0", MFI)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[mfid] bind :{MFI}: {e}");
            return 1;
        }
    };
    println!("[mfid] listening on :{MFI}");
    listen(listener, chip);
    0
}
