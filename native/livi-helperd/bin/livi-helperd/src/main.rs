use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux_main;
// The wired watcher drives a phone on this machine's USB or one on a LIVI Link dongle.
mod link;
mod wired;
#[cfg(target_os = "linux")]
mod aa;
#[cfg(target_os = "macos")]
mod mac_main;

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        if std::env::args().any(|a| a == "--wifi-ap-teardown") {
            return linux_main::run_wifi_ap_teardown();
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
