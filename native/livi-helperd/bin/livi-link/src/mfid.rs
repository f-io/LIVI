pub use iap2_mfi::server::PORT;

#[cfg(target_os = "linux")]
pub fn run(args: &[String]) -> std::process::ExitCode {
    use iap2_mfi::{I2cCoprocessor, server::listen, server::protocol_major};
    use std::net::TcpListener;

    // The bus, as `/dev/i2c-<n>` or a bare number. Chip is externally
    // powered, no GPIO — hence `-1`.
    let bus = args
        .first()
        .map(|a| a.trim_start_matches("/dev/i2c-"))
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(1);
    let mut chip = match I2cCoprocessor::open(bus, -1) {
        Ok(chip) => chip,
        Err(e) => {
            eprintln!("[mfid] no MFi chip on i2c-{bus}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let address = chip.address();
    let major = protocol_major(&mut chip)
        .map(|m| m.to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("[mfid] MFi @0x{address:02X} on i2c-{bus}, protocol major {major}");

    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[mfid] bind :{PORT}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("[mfid] listening on :{PORT}");
    listen(listener, chip);
    std::process::ExitCode::SUCCESS
}
