// Provisions a dongle as LIVI Link without a UI. The app drives the same crate.
// Host: $LIVI_LINK_HOST (default 10.10.10.1). The stack it installs is baked into this binary.
// A dongle of a project without a ready-made bootstrap gets one made from the vendor's own update
// for it, downloaded on the way. $LIVI_LINK_OTA names a file to use instead, for a machine that
// cannot reach the vendor's server.

mod bootstrap;

use std::path::{Path, PathBuf};
use std::time::Duration;

use livi_link_provision::dongle::arm::ax520;
use livi_link_provision::dongle::arm::imx6ul::payload::parts;
use livi_link_provision::dongle::arm::imx6ul::shell::{self, DEFAULT_HOST, Shell};
use livi_link_provision::dongle::arm::imx6ul::{Plan, Report, Status, apply, mtd, plan, verify};
use livi_link_provision::dongle::hook;
use livi_link_provision::dongle::probe::{Family, Probe};
use livi_link_provision::dongle::riscv::v821b;
use livi_link_provision::dongle::web::HostInfo;

const LEGACY_HOST: &str = "192.168.50.2";

/// The address to talk to. What the caller names wins, then the current one, then the old one.
fn pick_host() -> String {
    if let Ok(host) = std::env::var("LIVI_LINK_HOST") {
        return host;
    }
    for host in [DEFAULT_HOST, LEGACY_HOST] {
        if Shell::new(host).port_open(shell::TELNET_PORT) {
            return host.to_string();
        }
    }
    DEFAULT_HOST.to_string()
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

/// `dongle <arch> <soc> <command>`: architecture first, then the SoC, so every board is named by its
/// silicon.
fn run_dongle(args: &[String]) -> Result<bool, String> {
    let rest = args.get(2..).unwrap_or_default();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("arm"), Some("imx6ul")) => run_imx6ul(rest),
        (Some("arm"), Some("ax520")) => run_ax520(rest),
        (Some("riscv"), Some("v821b")) => run_v821b(rest),
        _ => Err(usage().to_string()),
    }
}

fn run_imx6ul(args: &[String]) -> Result<bool, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage().to_string());
    };
    let sh = Shell::new(&match command {
        "plan" | "apply" | "verify" | "backup" | "push" | "sh" => pick_host(),
        _ => DEFAULT_HOST.to_string(),
    });
    match command {
        "plan" => plan(&sh).map(|p| {
            print_plan(&p);
            true
        }),
        "apply" => {
            let reboot = args.iter().any(|a| a == "--reboot");
            apply(&sh, reboot, &|line| println!("== {line}")).map(|r| {
                print_report(&r);
                r.ok()
            })
        }
        "verify" => verify(&sh).map(|r| {
            print_report(&r);
            r.ok()
        }),
        "backup" => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(backup_dir);
            mtd::backup(&sh, &dir, &|line| println!("== {line}")).map(|dir| {
                println!("== backup in {}", livi_link_provision::tilde(&dir));
                true
            })
        }
        "push" => match &args[1..] {
            [local, remote] => std::fs::read(local)
                .map_err(|e| format!("{local}: {e}"))
                .and_then(|data| {
                    let md5 = livi_link_provision::dongle::arm::imx6ul::payload::md5_hex(&data);
                    sh.push(&data, remote, shell::PUSH_PORT, &md5).map(|()| {
                        println!("pushed {} bytes to {remote} ({md5})", data.len());
                        true
                    })
                }),
            _ => Err("usage: push <local> <remote>".to_string()),
        },
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
            let n = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(3_211_264);
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

fn run_v821b(args: &[String]) -> Result<bool, String> {
    match args.first().map(String::as_str) {
        Some("info") => stock_info(Some(v821b::PROJECT)),
        Some("install-shell") => stock_install_shell(Some(v821b::PROJECT)),
        Some("verify-hw") => v821b_verify(),
        Some("selftest") => {
            let n = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(3_211_264);
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

/// What a probe of the bus and the network turned up.
enum Found {
    StockCpc,
    Net(livi_link_provision::detect::Detected),
    Nothing,
}

/// Probes USB and the network at the same time. The first hit wins.
fn wait_for_dongle() -> Found {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};

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
    found
}

/// Started without arguments the tool asks rather than expecting commands. The subcommands stay
/// for scripting.
fn menu() -> std::process::ExitCode {
    use livi_link_provision::detect::Detected;
    loop {
        println!("\nsearching for a dongle (USB and network)…");
        let (stock_usb, detected) = match wait_for_dongle() {
            Found::StockCpc => (true, Detected::Nothing),
            Found::Net(d) => (false, d),
            Found::Nothing => (false, Detected::Nothing),
        };
        println!("\nLIVI Link provisioning tool v{VERSION}");
        if stock_usb {
            println!("Detected: CPC200-CCPA (stock, on USB — no shell yet)");
        } else {
            println!("Detected: {}", detected.label());
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
                None => println!("  1  open this dongle (patches the vendor's update for it, then looks at what it is)"),
            },
            Detected::LiviLink { .. } => {
                println!("  1  update LIVI Link");
            }
            Detected::Imx6ul { host } => {
                let sh = Shell::new(host);
                match action(&sh) {
                    Some(what) => println!("  1  {what} LIVI Link"),
                    None => println!("  r  reinstall LIVI Link"),
                }
            }
            Detected::Nothing if stock_usb => {
                println!("  1  bootstrap + install LIVI Link (over USB)");
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
            ("1", Detected::Imx6ul { host }) => {
                let sh = Shell::new(host);
                if action(&sh).is_some() {
                    match install(&sh) {
                        Ok(()) => return std::process::ExitCode::SUCCESS,
                        Err(e) => Err(e),
                    }
                } else {
                    Err("nothing to install".into())
                }
            }
            ("r", Detected::Imx6ul { host }) => {
                let sh = Shell::new(host);
                if action(&sh).is_none() {
                    match install(&sh) {
                        Ok(()) => return std::process::ExitCode::SUCCESS,
                        Err(e) => Err(e),
                    }
                } else {
                    Err("nothing to reinstall".into())
                }
            }
            ("1", Detected::Nothing) if stock_usb => match install(&Shell::new(DEFAULT_HOST)) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("1", Detected::LiviLink { .. }) => match install(&Shell::new(DEFAULT_HOST)) {
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

/// Whether this tool would change the dongle's firmware, and what that would be called.
fn action(sh: &Shell) -> Option<&'static str> {
    use livi_link_provision::dongle::arm::imx6ul::payload;
    if is_stock(sh).unwrap_or(true) {
        return Some("install");
    }
    let installed = installed_version(sh)?;
    let ours = payload::current_version();
    (payload::parts(&installed).1 != payload::parts(&ours).1).then_some("update")
}

/// The version on the dongle, if it carries one.
fn installed_version(sh: &Shell) -> Option<String> {
    let out = sh
        .sh(&format!("cat {} 2>/dev/null", livi_link_provision::dongle::arm::imx6ul::payload::VERSION_FILE))
        .unwrap_or_default()
        .trim()
        .to_string();
    (!out.is_empty()).then_some(out)
}

/// The whole job in one go: a shell if the dongle has none, then the backup, then the install.
/// It refuses to strip a dongle whose original is not saved, because that is the way back.
fn install(sh: &Shell) -> Result<(), String> {
    // A stock dongle offers the host no network, so the bootstrap rides into the next boot.
    if !sh.port_open(shell::TELNET_PORT) {
        println!("== this dongle has no way in yet, so it needs one unplug and plug back in");
        println!("== writing the bootstrap over USB");
        bootstrap::boot_hook()?;
        ask("unplug the dongle, plug it back in, then press enter")?;
        println!("== waiting, it takes about half a minute after the dongle has booted");
        wait_for_shell(sh)?;
    }
    // Whoever put it there, it goes before the backup, so the image is the dongle's own again.
    if sh.sh(&format!("[ -e {} ] && echo yes || echo no", bootstrap::BOOT_HOOK))?.trim() == "yes" {
        sh.push(
            bootstrap::carrier_body().as_bytes(),
            bootstrap::CARRIER,
            shell::PUSH_PORT,
            &livi_link_provision::dongle::arm::imx6ul::payload::md5_hex(bootstrap::carrier_body().as_bytes()),
        )?;
        sh.sh(&format!("chmod 755 {}; rm -f {}; sync", bootstrap::CARRIER, bootstrap::BOOT_HOOK))?;
        println!("== bootstrap removed again");
    }

    // From here on the dongle is being written to and must not be unplugged, so it says so.
    let blinking = blink(sh);

    // Only while the dongle is untouched. A backup of an already installed one is worthless and
    // would sit next to the real one, inviting a restore of the wrong image.
    if is_stock(sh)? {
        let dir = mtd::backup(sh, &backup_dir(), &report)?;
        println!("== backup in {}", livi_link_provision::tilde(&dir));
    } else {
        println!("== already installed, keeping the backup from the first time");
    }

    // Installed over whichever way in we had, but afterwards the dongle is LIVI Link and answers
    // over USB, so the restart and the check happen there.
    apply(sh, false, &report)?;
    drop(blinking);
    report("rebooting");
    sh.sh("sync; (sleep 1; reboot) >/dev/null 2>&1 &")?;
    std::thread::sleep(Duration::from_secs(5));
    let link = Shell::new(DEFAULT_HOST);
    wait_for_shell(&link)?;
    let outcome = verify(&link)?;
    print_report(&outcome);
    if outcome.ok() {
        let now = installed_version(&link).unwrap_or_else(|| "?".into());
        println!("\n== done, the dongle runs LIVI Link {} and is safe to unplug", parts(&now).0);
        Ok(())
    } else {
        Err("the dongle did not come back as expected".into())
    }
}

/// Alternates the two LEDs, the signal the vendor's updater gives while it writes. It runs
/// detached on the dongle and is stopped again however the install ends.
struct Blink<'a>(&'a Shell);

/// The loop itself. One line, because the shell on the dongle reads commands by line.
const BLINK_LOOP: &str = "echo $$ > /tmp/livi-blink.pid; \
                for g in 2 9; do \
                  [ -e /sys/class/gpio/gpio$g ] || echo $g > /sys/class/gpio/export; \
                  echo out > /sys/class/gpio/gpio$g/direction; \
                done; \
                while :; do \
                  echo 0 > /sys/class/gpio/gpio2/value; echo 1 > /sys/class/gpio/gpio9/value; sleep 0.25; \
                  echo 1 > /sys/class/gpio/gpio2/value; echo 0 > /sys/class/gpio/gpio9/value; sleep 0.25; \
                done";

fn blink(sh: &Shell) -> Blink<'_> {
    let _ = sh.sh(&format!("setsid sh -c '{BLINK_LOOP}' </dev/null >/dev/null 2>&1 &"));
    Blink(sh)
}

impl Drop for Blink<'_> {
    fn drop(&mut self) {
        // By its pid, because a pattern would match the shell that does the killing. Then back to
        // the steady red of normal operation.
        let _ = self.0.sh(
            "kill $(cat /tmp/livi-blink.pid 2>/dev/null) 2>/dev/null; rm -f /tmp/livi-blink.pid; \
             echo 1 > /sys/class/gpio/gpio9/value 2>/dev/null; \
             echo 0 > /sys/class/gpio/gpio2/value 2>/dev/null",
        );
    }
}

/// Whether the dongle still boots the vendor's script rather than ours.
fn is_stock(sh: &Shell) -> Result<bool, String> {
    let out = sh.sh(&format!(
        "grep -q '{}' {} 2>/dev/null && echo ours || echo stock",
        livi_link_provision::dongle::arm::imx6ul::payload::BRINGUP_MARKER,
        livi_link_provision::dongle::arm::imx6ul::payload::BRINGUP_REMOTE
    ))?;
    Ok(out.trim() == "stock")
}

/// Waits for the shell the bootstrap brings up.
fn wait_for_shell(sh: &Shell) -> Result<(), String> {
    for _ in 0..60 {
        if sh.port_open(shell::TELNET_PORT) {
            println!("== shell is up");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("no shell after two minutes, see LIVI-LINK.md".into())
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

fn usage() -> &'static str {
    "usage: livi-link-provision [detect]
  dongle arm imx6ul  plan | apply [--reboot] | verify | backup [dir] | push <local> <remote> | sh 'CMD' | bootstrap | usbscan
  dongle arm ax520   info | install-shell | verify-hw | selftest [N] | backup [dir] | flash <lfwb> | provision [lfwb]
  dongle riscv v821b info | install-shell | verify-hw | selftest [N] | backup [dir] | flash <lfwb> | provision [lfwb]"
}

/// Where backups go when no directory is given: the app's backup folder, the one that also
/// carries the config.json mirror — so copying it moves everything irreplaceable at once.
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

fn print_plan(p: &Plan) {
    println!("== dongle: {}", p.identity);
    println!("== rootfs free: {}", kib(p.free_k));
    let raw_k: u64 = p.delete.iter().map(|(_, size)| size / 1024).sum();
    println!("== ballast to delete ({} files, {raw_k}K raw):", p.delete.len());
    for (path, size) in &p.delete {
        println!("   {:>8}K  {path}", size / 1024);
    }
    if !p.keep_libs.is_empty() {
        println!("== ballast libs kept — referenced by a kept ELF:");
        for lib in &p.keep_libs {
            println!("   keep      {lib}");
        }
    }
    println!("== files:");
    for f in &p.files {
        println!("   {:<7} {:>7}K  {}", f.status.label(), f.bytes / 1024, f.remote);
    }
    let todo = p.files.iter().filter(|f| f.status != Status::Current).count();
    println!("== to push: {todo} files, {}K", p.push_k());
}

fn print_report(r: &Report) {
    for check in &r.checks {
        println!("   {}  {}", if check.ok { "ok " } else { "BAD" }, check.what);
    }
    println!(
        "   pair records: {}",
        if r.pair_records.is_empty() { "(none)" } else { &r.pair_records }
    );
    println!("   rootfs free: {}", kib(r.free_k));
    println!("{}", if r.ok() { "== VERIFIED" } else { "== PROBLEMS — see BAD lines above" });
}

fn kib(v: Option<u64>) -> String {
    v.map(|k| format!("{k}K")).unwrap_or_else(|| "unknown".into())
}

/// Only the V821B and the AX520 have a LIVI Link image, every other dongle stops at the bind-shell.
/// The family comes from the project when it is one we know. For any other project it comes from
/// what the shell on the dongle says, once the dongle has one.
fn family_of(info: &HostInfo) -> Option<Family> {
    match hook::ly_project(&info.sys.appver).as_deref() {
        Some(p) if p == v821b::PROJECT => Some(Family::V821b),
        Some(p) if p == ax520::PROJECT => Some(Family::Ax520),
        _ => probe_shell().ok().and_then(|probe| probe.family()),
    }
}

/// What the dongle's shell says it is.
fn probe_shell() -> Result<Probe, String> {
    if !livi_link_provision::dongle::shell::is_up() {
        return Err("the dongle has no shell yet".into());
    }
    let mut sh = livi_link_provision::dongle::shell::BindShell::connect(30)?;
    Probe::run(&mut sh)
}

/// Where the vendor's update for a project is kept between runs.
fn ota_cache_dir() -> PathBuf {
    backup_dir().with_file_name("ota-cache")
}

/// A dongle of a project we have no ready-made bootstrap for: get a shell onto it through the
/// vendor's own update, patched from the vendor's download, and only then look at what it is. The
/// dongle ends up on the vendor's public update for its project, with a shell open on 2323.
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

/// `expected` is the hardware family the caller named, a dongle of another family is reported.
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
        return Err("hardware verify failed — not touching mtd. bind-shell stays open for you.".into());
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
        return Err("hardware verify failed — not touching mtd. bind-shell stays open for you.".into());
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_blink_loop_stays_on_one_line() {
        assert!(!super::BLINK_LOOP.contains('\n'));
        assert!(!super::BLINK_LOOP.contains('\''));
        assert!(super::BLINK_LOOP.contains("gpio2/value"));
    }
}
