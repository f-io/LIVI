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

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        if std::env::args().any(|a| a == "--wifi-ap-status") {
            return linux_main::run_wifi_ap_status();
        }
        if std::env::args().any(|a| a == "--wifi-channels") {
            return livi_wifi::run();
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
