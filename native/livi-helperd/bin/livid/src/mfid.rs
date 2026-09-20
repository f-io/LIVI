use std::net::TcpListener;

use iap2_mfi::I2cCoprocessor;
use iap2_mfi::server::{PORT, listen, protocol_major};

pub fn run(args: Vec<String>) -> i32 {
    let bus: u32 = args
        .first()
        .map(|a| a.trim_start_matches("/dev/i2c-"))
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(1);
    let power_gpio: i32 = std::env::var("LIVI_MFI_POWER_GPIO")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1);

    let mut chip = match I2cCoprocessor::open(bus, power_gpio) {
        Ok(chip) => chip,
        Err(e) => {
            eprintln!("[mfid] no MFi chip on i2c-{bus}: {e}");
            return 1;
        }
    };
    let addr = chip.address();
    let major = protocol_major(&mut chip)
        .map(|m| m.to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("[mfid] MFi @0x{addr:02X} on i2c-{bus}, protocol major {major}");

    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[mfid] bind :{PORT}: {e}");
            return 1;
        }
    };
    println!("[mfid] listening on :{PORT}");
    listen(listener, chip);
    0
}
