// Provisions a CarlinKit dongle as LIVI Link without a UI; the app drives the same crate.
// Host: $LIVI_LINK_HOST (default 10.10.10.1). The stack it installs is baked into this binary.

mod bootstrap;

use std::path::{Path, PathBuf};
use std::time::Duration;

use livi_link_provision::payload::parts;
use livi_link_provision::shell::{self, DEFAULT_HOST, Shell};
use livi_link_provision::{Plan, Report, Status, apply, mtd, plan, verify};

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

    let sh = Shell::new(&match command {
        "plan" | "apply" | "verify" | "backup" | "push" | "sh" => pick_host(),
        _ => DEFAULT_HOST.to_string(),
    });

    let result = match command {
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
                    let md5 = livi_link_provision::payload::md5_hex(&data);
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
        "detect" => {
            let d = livi_link_provision::detect::detect();
            println!("{}", d.label());
            Ok(true)
        }
        "usbscan" => {
            for (v, p, name) in bootstrap::scan() {
                println!("{v:04x}:{p:04x}  {name}");
            }
            Ok(true)
        }
        "v821b" => match args.get(1).map(String::as_str) {
            Some("detect") => v821b_detect(),
            Some("install-shell") => v821b_install_shell(),
            Some("verify-hw") => v821b_verify(),
            Some("selftest") => {
                let n = args
                    .get(2)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(3_211_264);
                v821b_selftest(n)
            }
            Some("backup") => {
                let dir = args
                    .get(2)
                    .map(PathBuf::from)
                    .unwrap_or_else(backup_dir);
                v821b_backup(&dir)
            }
            Some("flash") => match args.get(2) {
                Some(path) => v821b_flash(&PathBuf::from(path)),
                None => Err("usage: v821b flash <path.lfwb>".to_string()),
            },
            Some("provision") => match args.get(2) {
                Some(path) => v821b_provision(Some(&PathBuf::from(path))),
                None => v821b_provision(None),
            },
            _ => Err("v821b: detect | install-shell | verify-hw | selftest [N] | backup [dir] | flash <lfwb> | provision [lfwb]".to_string()),
        }
        .map(|_| true),
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

        match &detected {
            Detected::V821bStock { .. } => {
                println!("  1  provision LIVI Link (backup current firmware first)");
            }
            Detected::V821bLink => {
                println!("  (already running LIVI Link — use its web UI to reflash)");
            }
            Detected::Cpc200 { host } => {
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
            ("1", Detected::V821bStock { .. }) => match v821b_provision(None) {
                Ok(()) => return std::process::ExitCode::SUCCESS,
                Err(e) => Err(e),
            },
            ("1", Detected::Cpc200 { host }) => {
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
            ("r", Detected::Cpc200 { host }) => {
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
    use livi_link_provision::payload;
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
        .sh(&format!("cat {} 2>/dev/null", livi_link_provision::payload::VERSION_FILE))
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
            &livi_link_provision::payload::md5_hex(bootstrap::carrier_body().as_bytes()),
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
        livi_link_provision::payload::BRINGUP_MARKER,
        livi_link_provision::payload::BRINGUP_REMOTE
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
    "usage: livi-link-provision plan | apply [--reboot] | verify | backup [dir] | push <local> <remote> | sh 'CMD'"
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

fn v821b_detect() -> Result<(), String> {
    let info = livi_link_provision::v821b::web::host()?;
    println!("name:    {}", info.name);
    println!("appver:  {}", info.sys.appver);
    println!("sn:      {}", info.sn);
    println!("wifi:    {}", info.wifi);
    println!("otp:     {}", info.otp);
    println!("led:     {}", info.sys.led);
    println!("update:  {}", info.update);
    Ok(())
}

fn v821b_install_shell() -> Result<(), String> {
    livi_link_provision::v821b::flash::install_bindshell()?;
    println!("done — bind-shell should come up on 2323 shortly");
    Ok(())
}

fn v821b_verify() -> Result<(), String> {
    let mut sh = livi_link_provision::v821b::shell::BindShell::connect(180)?;
    let hw = livi_link_provision::v821b::flash::verify_hardware(&mut sh)?;
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
    let mut sh = livi_link_provision::v821b::shell::BindShell::connect(180)?;
    livi_link_provision::v821b::flash::stream_in_selftest(&mut sh, size)
}

fn v821b_backup(out_dir: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::v821b::shell::BindShell::connect(180)?;
    livi_link_provision::v821b::flash::backup_stock(&mut sh, out_dir)?;
    Ok(())
}

fn v821b_flash(lfwb: &Path) -> Result<(), String> {
    let mut sh = livi_link_provision::v821b::shell::BindShell::connect(180)?;
    livi_link_provision::v821b::flash::flash_lfwb(&mut sh, lfwb)?;
    Ok(())
}

fn v821b_provision(lfwb: Option<&PathBuf>) -> Result<(), String> {
    livi_link_provision::v821b::flash::install_bindshell()?;
    let mut sh = livi_link_provision::v821b::shell::BindShell::connect(300)?;
    let hw = livi_link_provision::v821b::flash::verify_hardware(&mut sh)?;
    if !hw.looks_like_v821b_aic8800d80() {
        return Err("hardware verify failed — not touching mtd. bind-shell stays open for you.".into());
    }
    println!("hw: V821B+AIC8800D80 ✓  → backup + flash");
    let dir = backup_dir();
    livi_link_provision::v821b::flash::backup_stock(&mut sh, &dir)?;
    match lfwb {
        Some(path) => livi_link_provision::v821b::flash::flash_lfwb(&mut sh, path)?,
        None => livi_link_provision::v821b::flash::flash_embedded(&mut sh)?,
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
