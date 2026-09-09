// The dongle stack in one binary: it picks its job from argv[0], and livi-link.sh links it under
// each tool name.

#[cfg(target_os = "linux")]
mod bt;
#[cfg(target_os = "linux")]
mod iapd;
#[cfg(target_os = "linux")]
mod l2fwd;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod mdns;
#[cfg(target_os = "linux")]
mod mdnsd;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod mfid;
#[cfg(target_os = "linux")]
mod mgmt;
#[cfg(target_os = "linux")]
mod sdp;
#[cfg(target_os = "linux")]
mod seedrng;
mod usbproxy;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod wifi;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod wifid;

use std::path::Path;
use std::process::ExitCode;

/// The names the stack runs under, and the symlinks `livi-link.sh` creates for them.
const TOOLS: [&str; 8] = [
    "seedrng",
    "mfid",
    "livi-usbproxy",
    "l2fwd",
    "mdnsd",
    "wifid",
    "btd",
    "iapd",
];
const COMMANDS: [&str; 4] = ["wifi-channels", "bt-probe", "bt-mgmt", "sdp-dump"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let arg0 = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Started through its symlink, or as `livi-link <tool> ...` for a hand-run.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    let (tool, rest) = if TOOLS.contains(&arg0.as_str()) {
        (arg0, args[1..].to_vec())
    } else {
        (
            args.get(1).cloned().unwrap_or_default(),
            args.get(2..).unwrap_or_default().to_vec(),
        )
    };

    match tool.as_str() {
        "livi-usbproxy" | "usbproxy" => usbproxy::run(),
        #[cfg(target_os = "linux")]
        "seedrng" => seedrng::run(),
        #[cfg(target_os = "linux")]
        "mfid" => mfid::run(&rest),
        #[cfg(target_os = "linux")]
        "l2fwd" => l2fwd::run(&rest),
        #[cfg(target_os = "linux")]
        "mdnsd" => mdnsd::run(&rest),
        #[cfg(target_os = "linux")]
        "wifid" => wifid::run(),
        #[cfg(target_os = "linux")]
        "btd" => bt::run(),
        #[cfg(target_os = "linux")]
        "iapd" => iapd::run(&rest),
        #[cfg(target_os = "linux")]
        "wifi-channels" => wifi::run(),
        #[cfg(target_os = "linux")]
        "bt-probe" => bt::probe(),
        #[cfg(target_os = "linux")]
        "bt-mgmt" => mgmt::probe(),
        #[cfg(target_os = "linux")]
        "sdp-dump" => {
            sdp::dump();
            ExitCode::SUCCESS
        }
        other => {
            if !other.is_empty() && (TOOLS.contains(&other) || COMMANDS.contains(&other)) {
                eprintln!("livi-link: {other} runs on the dongle (linux) only");
            } else {
                eprintln!(
                    "usage: livi-link <{}|{}> [args]",
                    TOOLS.join("|"),
                    COMMANDS.join("|")
                );
            }
            ExitCode::from(2)
        }
    }
}
