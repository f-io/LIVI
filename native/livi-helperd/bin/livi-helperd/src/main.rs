use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux_main;
// The wired watcher drives a phone on this machine's USB or one on a LIVI Link dongle.
#[cfg(target_os = "linux")]
mod aa;
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

fn unknown_switch<I: Iterator<Item = String>>(args: I) -> Option<String> {
    args.take_while(|a| a.starts_with("--"))
        .find(|a| !SWITCHES.contains(&a.as_str()))
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
        linux_main::run()
    }
    #[cfg(target_os = "macos")]
    {
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
        assert_eq!(unknown_switch(args(&["--from-a-newer-caller"])), Some("--from-a-newer-caller".into()));
    }
}
