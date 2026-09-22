// Reads the MFi coprocessor's certificate and signs one challenge.
// Local mode: opens /dev/i2c-N directly.
// Remote mode: talks to a dongle's `mfid` over TCP.
//
// Usage:
//   mfi-probe [--bus N] [--power-gpio N] [--no-power]     # local
//   mfi-probe --remote host[:port]                        # via LIVI Link
//   mfi-probe --dongle                                    # shortcut for --remote livi-link.local:5000

use std::process::ExitCode;

use iap2_mfi::AuthCoprocessor;

enum Mode {
    Local { bus: u32, power_gpio: i32 },
    Remote { addr: String },
}

fn parse() -> Result<Mode, String> {
    let mut bus: u32 = 2;
    let mut power_gpio: i32 = -1;
    let mut remote: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bus"        => bus = args.next().and_then(|v| v.parse().ok()).unwrap_or(bus),
            "--power-gpio" => power_gpio = args.next().and_then(|v| v.parse().ok()).unwrap_or(power_gpio),
            "--no-power"   => power_gpio = -1,
            "--remote"     => remote = args.next(),
            "--dongle"     => remote = Some("livi-link.local:5000".into()),
            other          => return Err(format!("unknown argument: {other}")),
        }
    }
    match remote {
        Some(addr) => Ok(Mode::Remote {
            addr: if addr.contains(':') { addr } else { format!("{addr}:5000") },
        }),
        None => Ok(Mode::Local { bus, power_gpio }),
    }
}

fn drive(chip: &mut dyn AuthCoprocessor) -> Result<(), Box<dyn std::error::Error>> {
    let major = chip.protocol_major()?;
    println!("[mfi-probe] protocol_major={major}");

    let cert = chip.read_certificate()?;
    println!("[mfi-probe] certificate {} bytes", cert.len());
    println!("{}", hex(&cert));

    let challenge = vec![0xAB; if major >= 3 { 32 } else { 20 }];
    println!("[mfi-probe] signing {}-byte challenge", challenge.len());
    let response = chip.generate_challenge_response(&challenge)?;
    println!("[mfi-probe] signature {} bytes", response.len());
    println!("{}", hex(&response));
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match parse()? {
        Mode::Remote { addr } => {
            println!("[mfi-probe] remote {addr}");
            let mut chip = iap2_mfi::NcmCoprocessor::connect(&addr)?;
            drive(&mut chip)
        }
        #[cfg(target_os = "linux")]
        Mode::Local { bus, power_gpio } => {
            println!("[mfi-probe] local bus={bus} power_gpio={power_gpio}");
            let mut chip = iap2_mfi::I2cCoprocessor::open(bus, power_gpio)?;
            println!(
                "[mfi-probe] addr=0x{:02X} device_version=0x{:02X}",
                chip.address(), chip.device_version()?,
            );
            drive(&mut chip)
        }
        #[cfg(not(target_os = "linux"))]
        Mode::Local { .. } => {
            Err("local mode needs an i2c bus; on macOS use --remote or --dongle".into())
        }
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[mfi-probe] error: {e}");
            ExitCode::FAILURE
        }
    }
}
