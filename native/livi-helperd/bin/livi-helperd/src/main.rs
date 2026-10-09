use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux_main;
// The wired watcher drives a phone on this machine's USB or one on a LIVI Link dongle.
mod aa;
#[cfg(target_os = "macos")]
mod bt_dongle;
mod link;
#[cfg(target_os = "macos")]
mod mac_main;
mod wired;

/// Every switch this binary answers to.
const SWITCHES: &[&str] = &[
    "--wifi-ap-status",
    "--wifi-channels",
    "--install-wifi-ap",
    "--install-udev-rule",
    "--install-gvfs-guard",
    "--wifi-ap-claim",
    "--wifi-ap-teardown",
    "--bt-tunnel",
    "--wifi-ap",
];

/// Ctrl-C or the TERM systemd/Electron sends when LIVI stops.
#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn shutdown_signal() {
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

/// Core starts the helper with a pipe as stdin. It ends when core dies, even on
/// SIGKILL, and the helper then cleans up as on TERM.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn leave_with_core() {
    if !std::env::var("LIVI_LIFELINE").is_ok_and(|v| v == "1") {
        return;
    }
    let _ = std::thread::Builder::new().name("lifeline".into()).spawn(|| {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        // Core read stdout and stderr. With core gone every print would panic.
        if let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            use std::os::fd::AsRawFd;
            // SAFETY: both descriptors stay open, only their target changes.
            unsafe {
                libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO);
                libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO);
            }
        }
        // SAFETY: a signal to this process.
        unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
    });
}

fn unknown_switch<I: Iterator<Item = String>>(args: I) -> Option<String> {
    args.take_while(|a| a.starts_with("--")).find(|a| !SWITCHES.contains(&a.as_str()))
}

fn main() -> ExitCode {
    if let Some(bad) = unknown_switch(std::env::args().skip(1)) {
        eprintln!("livi-helperd: unknown switch {bad}");
        eprintln!("livi-helperd: known switches are {}", SWITCHES.join(", "));
        return ExitCode::FAILURE;
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::args().any(|a| a == "--wifi-ap-status") {
            return linux_main::run_wifi_ap_status();
        }
        if std::env::args().any(|a| a == "--wifi-channels") {
            return livi_wifi::run();
        }
        if let Some(at) = std::env::args().position(|a| a == "--install-wifi-ap") {
            let mut rest = std::env::args().skip(at + 1);
            return linux_main::run_install_wifi_ap(rest.next(), rest.next());
        }
        if let Some(at) = std::env::args().position(|a| a == "--install-udev-rule") {
            let mut rest = std::env::args().skip(at + 1);
            return linux_main::run_install_udev_rule(rest.next(), rest.next());
        }
        if let Some(at) = std::env::args().position(|a| a == "--install-gvfs-guard") {
            let mut rest = std::env::args().skip(at + 1);
            return linux_main::run_install_gvfs_guard(rest.next(), rest.next());
        }
        if std::env::args().any(|a| a == "--wifi-ap-claim") {
            return linux_main::run_wifi_ap_claim();
        }
        if std::env::args().any(|a| a == "--wifi-ap-teardown") {
            return linux_main::run_wifi_ap_teardown();
        }
        if std::env::args().any(|a| a == "--bt-tunnel") {
            return linux_main::run_bt_tunnel();
        }
        if std::env::args().any(|a| a == "--wifi-ap") {
            return linux_main::run_wifi_ap();
        }
        leave_with_core();
        linux_main::run()
    }
    #[cfg(target_os = "macos")]
    {
        leave_with_core();
        mac_main::run()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        eprintln!("livi-helperd runs on Linux and macOS");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::unknown_switch;

    fn args(list: &[&str]) -> std::vec::IntoIter<String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn a_known_switch_passes() {
        assert_eq!(unknown_switch(args(&["--wifi-ap"])), None);
        assert_eq!(unknown_switch(args(&["--install-wifi-ap", "/tmp/a", "/tmp/b"])), None);
    }

    #[test]
    fn no_switch_at_all_passes_and_runs_the_daemon() {
        assert_eq!(unknown_switch(args(&[])), None);
    }

    #[test]
    fn a_switch_this_build_does_not_know_is_named() {
        assert_eq!(
            unknown_switch(args(&["--from-a-newer-caller"])),
            Some("--from-a-newer-caller".into())
        );
    }
}
