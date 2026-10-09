// $LIVI_LINK_HOST sets the host (default 10.10.10.1), $LIVI_LINK_OTA names a vendor update file
// for a machine that cannot reach the vendor's server.

mod bootstrap;
mod wire;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use livi_link_provision::dongle::arm::ax520;
use livi_link_provision::dongle::arm::imx6ul::shell::{self, DEFAULT_HOST, Shell};
use livi_link_provision::dongle::arm::imx6ul::{self, boot, mtd};
use livi_link_provision::dongle::hook;
use livi_link_provision::dongle::link;
use livi_link_provision::dongle::mips::x1600;
use livi_link_provision::dongle::probe::{Family, Probe};
use livi_link_provision::dongle::rescue;
use livi_link_provision::dongle::riscv::v821b;
use livi_link_provision::dongle::web::HostInfo;

fn pick_host() -> String {
    std::env::var("LIVI_LINK_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string())
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        return menu();
    };

    let result = match command {
        "detect" => {
            let d = livi_link_provision::detect::detect();
            println!("{}", d.label());
            Ok(true)
        }
        "dongle" => run_dongle(&args[1..]),
        _ => {
            eprintln!("{}", usage());
            return std::process::ExitCode::from(2);
        }
    };

    match result {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// `dongle <arch> <soc> <command>`, so every board is named by its silicon.
fn run_dongle(args: &[String]) -> Result<bool, String> {
    let rest = args.get(2..).unwrap_or_default();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("arm"), Some("imx6ul")) => run_imx6ul(rest),
        (Some("arm"), Some("ax520")) => run_ax520(rest),
        (Some("riscv"), Some("v821b")) => run_v821b(rest),
        (Some("mips"), Some("x1600")) => run_x1600(rest),
        _ => Err(usage().to_string()),
    }
}

fn run_imx6ul(args: &[String]) -> Result<bool, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage().to_string());
    };
    let sh = Shell::new(&pick_host());
    match command {
        "backup" => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(backup_dir);
            mtd::backup(&sh, &dir, &|line| println!("== {line}")).map(|dir| {
                println!("== backup in {}", livi_link_provision::tilde(&dir));
                true
            })
        }
        "push" => match &args[1..] {
            [local, remote] => {
                std::fs::read(local).map_err(|e| format!("{local}: {e}")).and_then(|data| {
                    let md5 = shell::md5_hex(&data);
                    sh.push(&data, remote, shell::PUSH_PORT, &md5).map(|()| {
                        println!("pushed {} bytes to {remote} ({md5})", data.len());
                        true
                    })
                })
            }
            _ => Err("usage: push <local> <remote>".to_string()),
        },
        "kernel" => match &args[1..] {
            [zimage, rest @ ..] if rest.iter().all(|a| a == "--write") => {
                let data = std::fs::read(zimage).map_err(|e| format!("{zimage}: {e}"))?;
                let plan = boot::prepare(&sh, &data, &backup_dir(), &report)?;
                println!(
                    "== zImage {} of {} bytes U-Boot reads, staging at {:#x}",
                    data.len(),
                    plan.layout.kernel_len,
                    plan.layout.staging
                );
                println!(
                    "== {}",
                    if plan.unchanged {
                        "kernel partition already holds this, only the conversion is left"
                    } else {
                        "kernel partition differs and will be written"
                    }
                );
                if rest.is_empty() {
                    println!("== dry run, add --write to write and reboot");
                    return Ok(true);
                }
                boot::install(&sh, &plan, &report).map(|()| true)
            }
            _ => Err("usage: kernel <zImage> [--write]".to_string()),
        },
        "flash" => match &args[1..] {
            [lfwb, rest @ ..] if rest.iter().all(|a| a == "--write") => {
                let data = std::fs::read(lfwb).map_err(|e| format!("{lfwb}: {e}"))?;
                if rest.is_empty() {
                    let bundle = boot::read_bundle(&data)?;
                    let size = |i: &Option<Vec<u8>>| {
                        i.as_ref().map_or("none".into(), |d| format!("{} B", d.len()))
                    };
                    println!(
                        "== bundle: kernel {}, rootfs {}",
                        size(&bundle.kernel),
                        size(&bundle.rootfs)
                    );
                    println!("== the dongle runs: {:?}", imx6ul::running(&sh)?);
                    if let Some(kernel) = &bundle.kernel {
                        boot::prepare(&sh, kernel, &backup_dir(), &report)?;
                    }
                    println!("== dry run, add --write to install");
                    return Ok(true);
                }
                imx6ul::install(&sh, &data, &backup_dir(), &report).map(|()| true)
            }
            _ => Err("usage: flash <lfwb> [--write]".to_string()),
        },
        "provision" => imx6ul_provision(args.get(1).map(Path::new)).map(|()| true),
        "restore" => imx6ul_back_to_stock(args.get(1).map(Path::new)).map(|()| true),
        "bootstrap" => bootstrap::boot_hook().map(|()| {
            println!("bootstrap written, replug the dongle to start its shell");
            true
        }),
        "sh" => sh.run(&args[1..].join(" "), Duration::from_secs(120)).map(|out| {
            println!("{out}");
            true
        }),
        "usbscan" => {
            for (v, p, name) in bootstrap::scan() {
                println!("{v:04x}:{p:04x}  {name}");
            }
            Ok(true)
        }
        _ => Err(usage().to_string()),
    }
}

fn run_ax520(args: &[String]) -> Result<bool, String> {
    match args.first().map(String::as_str) {
        Some("info") => stock_info(Some(ax520::PROJECT)),
        Some("install-shell") => stock_install_shell(Some(ax520::PROJECT)),
        Some("verify-hw") => ax520_verify(),
        Some("selftest") => {
            let n = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(3_211_264);
            ax520_selftest(n)
        }
        Some("backup") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(backup_dir);
            ax520_backup(&dir)
        }
        Some("flash") => match args.get(1) {
            Some(path) => ax520_flash(&PathBuf::from(path)),
            None => Err("usage: dongle arm ax520 flash <path.lfwb>".to_string()),
        },
        Some("provision") => match args.get(1) {
            Some(path) => ax520_provision(Some(&PathBuf::from(path))),
            None => ax520_provision(None),
        },
        _ => Err(usage().to_string()),
    }
    .map(|_| true)
}

/// Identify, back up and flash through the vendor's own OTA, which keeps the other bank as the
/// way back. The host is the dongle's AP address unless $LIVI_LINK_HOST says otherwise.
fn run_x1600(args: &[String]) -> Result<bool, String> {
    let host = std::env::var("LIVI_LINK_HOST").unwrap_or_else(|_| x1600::default_host().into());
    match args.first().map(String::as_str) {
        Some("info") => {
            let v = x1600::version(&host)?;
            println!("model:    {}", v.model);
            println!("platform: {}", v.platform.as_deref().unwrap_or("?"));
            println!("custom:   {}", v.custom.as_deref().unwrap_or("?"));
            println!("system:   {}", v.system_version.as_deref().unwrap_or("?"));
            println!(
                "{}",
                if v.model == x1600::MODEL { "a supported X1600 dongle" } else { "NOT an X1600 dongle" }
            );
            Ok(true)
        }
        Some("fetch") => {
            let out = args.get(1).ok_or("usage: dongle mips x1600 fetch <out.img>")?;
            let ota = x1600::fetch(&x1600::version(&host)?)?;
            std::fs::write(out, &ota).map_err(|e| format!("{out}: {e}"))?;
            println!("vendor OTA saved to {out} ({} B)", ota.len());
            Ok(true)
        }
        Some("backup") => {
            let positional = args.get(1).filter(|a| !a.starts_with("--"));
            let dir = positional.map(PathBuf::from).unwrap_or_else(backup_dir);
            let mut sh = livi_link_provision::dongle::shell::BindShell::connect(30)?;
            match bank_flag(&args[1..])? {
                Some(bank) => x1600::backup_bank(&mut sh, bank, &dir).map(|_| true),
                None => x1600::backup_stock(&mut sh, &dir).map(|_| true),
            }
        }
        Some("flash") => {
            let path = args.get(1).ok_or("usage: dongle mips x1600 flash <ota.img> [--write] [--reboot]")?;
            let ota = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let parts = x1600::image::parse(&ota)?;
            println!(
                "== image ok: kernel {} B, rootfs {} B, checks match",
                parts.kernel.len(),
                parts.rootfs.len()
            );
            let flags = &args[2..];
            if !flags.iter().any(|a| a == "--write") {
                println!("== dry run, add --write to flash the inactive bank");
                return Ok(true);
            }
            x1600::flash(&host, &ota, &report)?;
            println!("== ota update ok, the other bank is now the next boot");
            if flags.iter().any(|a| a == "--reboot") {
                x1600::reboot(&host);
                println!("== rebooting");
            } else {
                println!("== reboot it when ready: http://{host}/cgi-bin/reboot.cgi");
            }
            Ok(true)
        }
        Some("install-shell") => {
            let path_arg = args.get(1).filter(|a| !a.starts_with("--"));
            let ota = match path_arg {
                Some(path) => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
                None => x1600::fetch(&x1600::version(&host)?)?,
            };
            let patched = x1600::shell::with_shell(&ota)?;
            let parts = x1600::image::parse(&patched)?;
            println!(
                "== patched image ready: kernel {} B, rootfs {} B, bind-shell on 2323",
                parts.kernel.len(),
                parts.rootfs.len()
            );
            let flags = &args[if path_arg.is_some() { 2 } else { 1 }..];
            if !flags.iter().any(|a| a == "--write") {
                println!("== dry run, add --write to flash the inactive bank");
                return Ok(true);
            }
            x1600::flash(&host, &patched, &report)?;
            println!("== ota update ok, the other bank is now the next boot, with the bind-shell on 2323");
            if flags.iter().any(|a| a == "--reboot") {
                x1600::reboot(&host);
                println!("== rebooting");
            } else {
                println!("== reboot it when ready: http://{host}/cgi-bin/reboot.cgi");
            }
            Ok(true)
        }
        _ => Err(usage().to_string()),
    }
}

fn run_v821b(args: &[String]) -> Result<bool, String> {
    match args.first().map(String::as_str) {
        Some("info") => stock_info(Some(v821b::PROJECT)),
        Some("install-shell") => stock_install_shell(Some(v821b::PROJECT)),
        Some("verify-hw") => v821b_verify(),
        Some("selftest") => {
            let n = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(3_211_264);
            v821b_selftest(n)
        }
        Some("backup") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(backup_dir);
            v821b_backup(&dir)
        }
        Some("flash") => match args.get(1) {
            Some(path) => v821b_flash(&PathBuf::from(path)),
            None => Err("usage: dongle riscv v821b flash <path.lfwb>".to_string()),
        },
        Some("provision") => match args.get(1) {
            Some(path) => v821b_provision(Some(&PathBuf::from(path))),
            None => v821b_provision(None),
        },
        _ => Err(usage().to_string()),
    }
    .map(|_| true)
}

const VERSION: &str = match option_env!("LIVI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

enum Found {
    StockCpc,
    Net(livi_link_provision::detect::Detected),
    LinkOnUsb(bootstrap::LinkOnUsb),
    Nothing,
}

fn wait_for_dongle() -> Found {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    let usb = {
        let stop = Arc::clone(&stop);
        let tx = tx.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if bootstrap::stock_dongle_once() {
                    let _ = tx.send(Found::StockCpc);
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    {
        let stop = Arc::clone(&stop);
        let tx = tx.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let d = livi_link_provision::detect::detect();
                if !matches!(d, livi_link_provision::detect::Detected::Nothing) {
                    let _ = tx.send(Found::Net(d));
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }
    drop(tx);

    let found = rx.recv_timeout(Duration::from_secs(60)).unwrap_or(Found::Nothing);
    stop.store(true, Ordering::Relaxed);
    let _ = usb.join();
    match found {
        Found::Nothing => bootstrap::livi_link_on_usb().map_or(Found::Nothing, Found::LinkOnUsb),
        found => found,
    }
}

fn menu() -> std::process::ExitCode {
    use livi_link_provision::detect::Detected;
    loop {
        println!("\nsearching for a dongle (USB and network)…");
        let (stock_usb, link_usb, detected) = match wait_for_dongle() {
            Found::StockCpc => (true, None, Detected::Nothing),
            Found::Net(d) => (false, None, d),
            Found::LinkOnUsb(usb) => (false, Some(usb), Detected::Nothing),
            Found::Nothing => (false, None, Detected::Nothing),
        };
        let wifi_only = link_usb == Some(bootstrap::LinkOnUsb::NoNcm);
        println!("\nLIVI Link provisioning tool v{VERSION}");
        if stock_usb {
            println!("Detected: i.MX6UL dongle in stock firmware, on USB (no shell yet)");
        } else {
            match link_usb {
                Some(bootstrap::LinkOnUsb::NoNcm) => println!(
                    "Detected: i.MX6UL dongle with the bootstrap, its kernel has no network over USB (no NCM)"
                ),
                Some(bootstrap::LinkOnUsb::Ncm) => {
                    println!("Detected: LIVI Link on USB, but this computer has no network to it");
                    println!(
                        "  check that its new network interface got an address next to {}",
                        shell::DEFAULT_HOST
                    );
                }
                None => println!("Detected: {}", detected.label()),
            }
        }

        let family = match &detected {
            Detected::DongleStock { info } => family_of(info),
            _ => None,
        };
        match &detected {
            Detected::DongleStock { .. } => match family {
                Some(_) => println!("  1  provision LIVI Link (backup current firmware first)"),
                None if livi_link_provision::dongle::shell::is_up() => {
                    println!("  1  look at this dongle's hardware (no LIVI Link image for it yet)");
                }
                None => println!(
                    "  1  open this dongle (patches the vendor's update for it, then looks at what it is)"
                ),
            },
            Detected::LiviLink { target, .. } => {
                println!("  1  update LIVI Link");
                if target.starts_with("imx6ul_") {
                    println!("  2  back to the vendor firmware (from backup)");
                }
            }
            Detected::Imx6ul { .. } => {
                println!("  1  install LIVI Link (backup current firmware first)");
                println!("  2  back to the vendor firmware (from backup)");
            }
            Detected::Rescue { .. } => println!("  1  write LIVI Link again"),
            Detected::X1600Stock { .. } => {
                println!("  (Ingenic X1600: use `dongle mips x1600 backup | flash`, no LIVI Link image yet)");
            }
            Detected::Nothing if stock_usb => {
                println!("  1  bootstrap + install LIVI Link (over USB)");
            }
            Detected::Nothing if wifi_only => {
                println!("  1  install LIVI Link over the dongle's Wi-Fi");
            }
            Detected::Nothing => {}
        }
        println!("  q  quit");
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return std::process::ExitCode::SUCCESS;
        }
        let outcome: Result<(), String> = match (line.trim(), &detected) {
            ("1", Detected::DongleStock { .. }) => {
                let done = match family {
                    Some(Family::V821b) => v821b_provision(None),
                    Some(Family::Ax520) => ax520_provision(None),
                    None => open_unknown(),
                };
                match (done, family) {
                    // Only looked at it, the next round offers what the hardware allows.
                    (Ok(()), None) => Ok(()),
                    (Ok(()), Some(_)) => return std::process::ExitCode::SUCCESS,
                    (Err(e), _) => Err(e),
                }
            }
            ("1", Detected::Imx6ul { .. }) => match imx6ul_provision(None) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("1", Detected::Nothing) if stock_usb || wifi_only => match imx6ul_provision(None) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("1", Detected::Rescue { family }) => match rescue_reflash(*family) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("2", Detected::LiviLink { target, .. }) if target.starts_with("imx6ul_") => {
                match imx6ul_back_to_stock(None) {
                    Ok(()) => return std::process::ExitCode::SUCCESS,
                    Err(e) => Err(e),
                }
            }
            ("2", Detected::Imx6ul { .. }) => match imx6ul_back_to_stock(None) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("1", Detected::LiviLink { .. }) => match update_livi_link() {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("q" | "quit" | "", _) => return std::process::ExitCode::SUCCESS,
            (other, _) => Err(format!("no such choice: {other}")),
        };
        if let Err(e) = outcome {
            eprintln!("error: {e}");
        }
    }
}

fn imx6ul_provision(lfwb: Option<&Path>) -> Result<(), String> {
    let given = lfwb
        .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .transpose()?;
    let host = pick_host();
    let sh = Shell::new(&host);
    // A stock dongle offers the host no network, so the bootstrap rides into the next boot.
    if !sh.reachable() {
        if bootstrap::livi_link_on_usb() != Some(bootstrap::LinkOnUsb::NoNcm) {
            println!("== this dongle has no way in yet, so it needs one unplug and plug back in");
            println!("== writing the bootstrap over USB");
            bootstrap::boot_hook()?;
            ask("unplug the dongle, plug it back in, then press enter")?;
            println!("== waiting, it takes about half a minute after the dongle has booted");
        }
        wait_for_shell(&sh)?;
    }
    // Whoever put it there, it goes before the backup, so the image is the dongle's own again.
    if sh.sh(&format!("[ -e {} ] && echo yes || echo no", bootstrap::BOOT_HOOK))?.trim() == "yes" {
        let carrier = bootstrap::carrier_body().as_bytes();
        sh.push(carrier, bootstrap::CARRIER, shell::PUSH_PORT, &shell::md5_hex(carrier))?;
        sh.sh(&format!("chmod 755 {}; rm -f {}; sync", bootstrap::CARRIER, bootstrap::BOOT_HOOK))?;
        println!("== bootstrap removed again");
    }
    let bundle = match given {
        Some(bundle) => bundle,
        None => imx6ul_bundle(&sh)?.to_vec(),
    };
    imx6ul::install(&sh, &bundle, &backup_dir(), &report)?;
    let now = wait_for_livi_link(&host)?;
    println!(
        "\n== done, the dongle runs LIVI Link {} ({}) and is safe to unplug",
        now.version, now.build
    );
    Ok(())
}

fn rescue_reflash(family: Family) -> Result<(), String> {
    let mut sh = Shell::new(DEFAULT_HOST);
    rescue::reflash(&mut sh, family)?;
    let now = wait_for_livi_link(DEFAULT_HOST)?;
    println!(
        "\n== done, the dongle runs LIVI Link {} ({}) and is safe to unplug",
        now.version, now.build
    );
    Ok(())
}

fn imx6ul_back_to_stock(dir: Option<&Path>) -> Result<(), String> {
    let sh = Shell::new(&pick_host());
    let stock = imx6ul::restore::find(&sh, &backup_dir(), dir, &report)?;
    println!("== the backup of this dongle: {}", livi_link_provision::tilde(&stock.dir));
    let answer = ask("write the vendor firmware back, LIVI Link is gone afterwards? [y/N]")?;
    if !answer.eq_ignore_ascii_case("y") {
        println!("== left as it is");
        return Ok(());
    }
    imx6ul::back_to_stock(&sh, &stock, &report)?;
    println!(
        "\n== done, the dongle starts its vendor firmware, leave it plugged in until it is up"
    );
    Ok(())
}

fn imx6ul_bundle(sh: &Shell) -> Result<&'static [u8], String> {
    let target = imx6ul::module_target(sh)?.unwrap_or_else(|| {
        println!(
            "== there is no build for this dongle's Wi-Fi module yet, the IW416 one runs all but Wi-Fi and Bluetooth"
        );
        "imx6ul_iw416"
    });
    println!("== firmware {target}");
    link::bundle(target).ok_or_else(|| {
        format!("no {target} firmware baked in, this is a local build without CI assets")
    })
}

fn update_livi_link() -> Result<(), String> {
    let host = pick_host();
    let before = link::status(&host).ok_or("the dongle does not answer as LIVI Link")?;
    let bundle = if before.target.starts_with("imx6ul_") {
        imx6ul_bundle(&Shell::new(&host))?
    } else {
        link::bundle(&before.target)
            .ok_or_else(|| format!("this tool carries no firmware for {}", before.target))?
    };
    println!(
        "== {} runs {} ({}), uploading {} bytes",
        before.model,
        before.version,
        before.build,
        bundle.len()
    );
    let done = match link::update(&host, bundle) {
        Ok(done) => done,
        Err(e) => {
            let typ = link::refused(&e).ok_or(e)?;
            if before.target.starts_with("imx6ul_") {
                println!(
                    "== the firmware on the dongle does not take this bundle yet, installing it from the rescue system"
                );
                return imx6ul_provision(None);
            }
            println!(
                "== the firmware on the dongle does not take image type {typ} yet, the rest of the bundle brings one that does"
            );
            let first = link::update(&host, &link::without(bundle, typ)?)?;
            println!("== {first}");
            if first.contains("rebooting") {
                back_after_reboot(&host)?;
            }
            println!("== uploading the whole bundle");
            link::update(&host, bundle)?
        }
    };
    println!("== {done}");
    if done.contains("rebooting") {
        let now = back_after_reboot(&host)?;
        println!("\n== done, the dongle runs LIVI Link {} ({})", now.version, now.build);
    }
    Ok(())
}

/// Waits for it to go first, so the system going down does not count as back.
fn back_after_reboot(host: &str) -> Result<link::Status, String> {
    let gone = Instant::now();
    while link::status(host).is_some() && gone.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_secs(1));
    }
    wait_for_livi_link(host)
}

/// On the i.MX6UL a new kernel takes a U-Boot conversion and two starts.
fn wait_for_livi_link(host: &str) -> Result<link::Status, String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(240) {
        if let Some(status) = link::status(host) {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("the dongle did not come back as LIVI Link within four minutes".into())
}

fn wait_for_shell(sh: &Shell) -> Result<(), String> {
    for _ in 0..60 {
        if sh.port_open(shell::TELNET_PORT) {
            println!("== shell is up");
            return Ok(());
        }
        if bootstrap::livi_link_on_usb() == Some(bootstrap::LinkOnUsb::NoNcm) {
            return shell_over_wifi(sh);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(match bootstrap::livi_link_on_usb() {
        Some(bootstrap::LinkOnUsb::Ncm) => format!(
            "the bootstrap is up and offers a network over USB, but this computer did not bring it \
             up, check that its new network interface got an address next to {}",
            shell::DEFAULT_HOST
        ),
        _ => "no shell after two minutes, see LIVI-LINK.md".into(),
    })
}

/// The bootstrap leaves the vendor's access point up where the kernel has no NCM.
fn shell_over_wifi(sh: &Shell) -> Result<(), String> {
    println!("== the dongle's kernel has no network over USB (no NCM), its Wi-Fi is the way in");
    ask("join the dongle's own Wi-Fi from this computer, then press enter")?;
    for _ in 0..30 {
        let answering = bootstrap::VENDOR_AP_HOSTS
            .into_iter()
            .find(|host| Shell::new(host).port_open(shell::TELNET_PORT));
        if let Some(host) = answering {
            sh.move_to(host);
            println!("== shell is up at {host}");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("no shell on the dongle's Wi-Fi either, see LIVI-LINK.md".into())
}

fn ask(what: &str) -> Result<String, String> {
    print!("{what}: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

fn report(line: &str) {
    println!("== {line}");
}

/// `--bank 1` or `--bank 2` out of a flag list, for `dongle mips x1600 backup`.
fn bank_flag(args: &[String]) -> Result<Option<u8>, String> {
    let Some(pos) = args.iter().position(|a| a == "--bank") else { return Ok(None) };
    let value = args.get(pos + 1).ok_or("--bank needs a 1 or a 2 after it")?;
    match value.as_str() {
        "1" => Ok(Some(1)),
        "2" => Ok(Some(2)),
        other => Err(format!("--bank must be 1 or 2, not {other:?}")),
    }
}

fn usage() -> &'static str {
    "usage: livi-link-provision [detect]
  dongle arm imx6ul  provision [lfwb] | restore [backup dir] | flash <lfwb> [--write] | kernel <zImage> [--write] | backup [dir] | push <local> <remote> | sh 'CMD' | bootstrap | usbscan
  dongle arm ax520   info | install-shell | verify-hw | selftest [N] | backup [dir] | flash <lfwb> | provision [lfwb]
  dongle mips x1600  info | fetch <out.img> | backup [dir] [--bank 1|2] | flash <ota.img> [--write] [--reboot] | install-shell [<ota.img>] [--write] [--reboot]
  dongle riscv v821b info | install-shell | verify-hw | selftest [N] | backup [dir] | flash <lfwb> | provision [lfwb]"
}

/// Next to the config.json mirror, so one copy saves everything irreplaceable.
fn backup_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let base = if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Application Support/LIVI/backup")
    } else {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join(".local/share"))
            .join("LIVI")
    };
    base.join("dongle-backup")
}

/// Only the V821B and the AX520 have a LIVI Link image.
fn family_of(info: &HostInfo) -> Option<Family> {
    match hook::ly_project(&info.sys.appver).as_deref() {
        Some(p) if p == v821b::PROJECT => Some(Family::V821b),
        Some(p) if p == ax520::PROJECT => Some(Family::Ax520),
        _ => probe_shell().ok().and_then(|probe| probe.family()),
    }
}

fn probe_shell() -> Result<Probe, String> {
    if !livi_link_provision::dongle::shell::is_up() {
        return Err("the dongle has no shell yet".into());
    }
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(30)?;
    Probe::run(&mut sh)
}

fn ota_cache_dir() -> PathBuf {
    backup_dir().with_file_name("ota-cache")
}

/// The dongle ends up on the vendor's public update with a shell on port 2323.
fn open_unknown() -> Result<(), String> {
    hook::install_bindshell(None, &ota_cache_dir())?;
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(300)?;
    let probe = Probe::run(&mut sh)?;
    let report = probe.report();
    println!("\n{report}");

    let dir = backup_dir().with_file_name("dongle-probe");
    let file = dir.join(format!("probe-{}.txt", livi_link_provision::dongle::lfwb::stamp()));
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, &report)) {
        Ok(()) => println!("== saved {}", livi_link_provision::tilde(&file)),
        Err(e) => println!("== could not save the report: {e}"),
    }
    match probe.family() {
        Some(family) => println!(
            "== this is a {}. Run the tool again to provision it, the current firmware is backed up first.",
            family.name()
        ),
        None => println!(
            "== this hardware is not one we have a LIVI Link image for yet, nothing more was done to it.\n\
             == Please attach the report to an issue. The dongle's own update page puts the vendor's firmware back."
        ),
    }
    Ok(())
}

/// `expected` is the hardware family the caller named.
fn stock_info(expected: Option<&str>) -> Result<(), String> {
    let info = livi_link_provision::dongle::web::host()?;
    println!("name:    {}", info.name);
    println!("appver:  {}", info.sys.appver);
    println!("sn:      {}", info.sn);
    println!("wifi:    {}", info.wifi);
    println!("otp:     {}", info.otp);
    println!("led:     {}", info.sys.led);
    println!("update:  {}", info.update);
    let project = livi_link_provision::dongle::hook::ly_project(&info.sys.appver);
    match &project {
        Some(p) => println!("project: {p}"),
        None => println!("project: (couldn't parse from appver)"),
    }
    println!(
        "bootstrap: made from the vendor's update for this project, {}",
        livi_link_provision::dongle::ota::version_url(&info.sys.appver)
            .unwrap_or_else(|| "no address for it".to_string())
    );
    match &project {
        Some(p) => hook::check_project(p, expected),
        None => Ok(()),
    }
}

fn stock_install_shell(expected: Option<&str>) -> Result<(), String> {
    hook::install_bindshell(expected, &ota_cache_dir())?;
    println!("done — bind-shell should come up on 2323 shortly");
    Ok(())
}

fn v821b_verify() -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    let hw = livi_link_provision::dongle::riscv::v821b::verify_hardware(&mut sh)?;
    println!("--- /proc/cpuinfo ---\n{}\n", hw.cpuinfo_head);
    println!("--- /proc/mtd ---\n{}\n", hw.proc_mtd);
    println!("--- aic8800 modules ---\n{}\n", hw.aic_modules);
    if hw.looks_like_v821b_aic8800d80() {
        println!("hardware: V821B + AIC8800D80 (as expected)");
        Ok(())
    } else {
        Err("hardware check failed — not a V821B+AIC8800D80".into())
    }
}

fn v821b_selftest(size: usize) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    livi_link_provision::dongle::riscv::v821b::stream_in_selftest(&mut sh, size)
}

fn v821b_backup(out_dir: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    livi_link_provision::dongle::riscv::v821b::backup_stock(&mut sh, out_dir)?;
    Ok(())
}

fn v821b_flash(lfwb: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    livi_link_provision::dongle::riscv::v821b::flash_lfwb(&mut sh, lfwb)?;
    Ok(())
}

fn v821b_provision(lfwb: Option<&PathBuf>) -> Result<(), String> {
    hook::install_bindshell(Some(v821b::PROJECT), &ota_cache_dir())?;
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(300)?;
    let hw = livi_link_provision::dongle::riscv::v821b::verify_hardware(&mut sh)?;
    if !hw.looks_like_v821b_aic8800d80() {
        return Err(
            "hardware verify failed — not touching mtd. bind-shell stays open for you.".into()
        );
    }
    println!("hw: V821B+AIC8800D80 ✓  → backup + flash");
    let dir = backup_dir();
    livi_link_provision::dongle::riscv::v821b::backup_stock(&mut sh, &dir)?;
    match lfwb {
        Some(path) => livi_link_provision::dongle::riscv::v821b::flash_lfwb(&mut sh, path)?,
        None => livi_link_provision::dongle::riscv::v821b::flash_embedded(&mut sh)?,
    }
    println!("provision complete — dongle rebooting into LIVI Link");
    Ok(())
}

fn ax520_verify() -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    let hw = ax520::verify_hardware(&mut sh)?;
    println!("--- /proc/cpuinfo ---\n{}\n", hw.cpuinfo_head);
    println!("--- /proc/mtd ---\n{}\n", hw.proc_mtd);
    if hw.looks_like_ax520_aic8800d80() {
        println!("hardware: AX520 + AIC8800D80 (as expected)");
        Ok(())
    } else {
        Err("hardware check failed — not an AX520 with the stock partition table".into())
    }
}

fn ax520_selftest(size: usize) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    ax520::stream_in_selftest(&mut sh, size)
}

fn ax520_backup(out_dir: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    ax520::backup_stock(&mut sh, out_dir)?;
    Ok(())
}

fn ax520_flash(lfwb: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(180)?;
    ax520::flash_lfwb(&mut sh, lfwb)?;
    Ok(())
}

fn ax520_provision(lfwb: Option<&PathBuf>) -> Result<(), String> {
    hook::install_bindshell(Some(ax520::PROJECT), &ota_cache_dir())?;
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(300)?;
    let hw = ax520::verify_hardware(&mut sh)?;
    if !hw.looks_like_ax520_aic8800d80() {
        return Err(
            "hardware verify failed — not touching mtd. bind-shell stays open for you.".into()
        );
    }
    println!("hw: AX520+AIC8800D80 ✓  → backup + flash");
    let dir = backup_dir();
    ax520::backup_stock(&mut sh, &dir)?;
    match lfwb {
        Some(path) => ax520::flash_lfwb(&mut sh, path)?,
        None => ax520::flash_embedded(&mut sh)?,
    }
    println!("provision complete — dongle rebooting into LIVI Link");
    Ok(())
}
