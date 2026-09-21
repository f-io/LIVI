// livid — multi-call binary for the LIVI-Link (V821B) dongle.
//
// Symlinks (livi-tinyshell, livi-netd, livi-httpd, livi-bt-up, livi-ledd)
// point at this binary; argv[0]'s basename selects the module. Every module
// exposes `pub fn run(args: Vec<String>) -> i32`. Sharing the Rust runtime
// across all tools cuts >3 MiB of duplicated allocator/panic/std code.

use std::env;
use std::path::Path;
use std::process::ExitCode;

mod bt_up;
mod btd;
mod config;
mod iapd;
mod ledd;
mod mfid;
mod netd;
mod tinyshell;
mod wifid;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let arg0 = args.first().map(|s| s.as_str()).unwrap_or("livid");
    let basename = Path::new(arg0).file_name().and_then(|s| s.to_str()).unwrap_or("livid");

    // If invoked as `livid <cmd> [args…]`, use the sub-command.
    // Otherwise dispatch on the symlink name.
    let (cmd, rest): (&str, Vec<String>) = if basename == "livid" {
        match args.get(1) {
            Some(sub) => (sub.as_str(), args[2..].to_vec()),
            None => {
                eprintln!("usage: livid <bt-up|bt-mgmt|bt-probe|btd|config|httpd|iapd|ledd|mfid|netd|tinyshell|wifid> [args…]");
                return ExitCode::from(2);
            }
        }
    } else {
        (basename, args[1..].to_vec())
    };

    let rc = match cmd {
        "livi-bt-up"     | "bt-up"     => bt_up::run(rest),
        // The same diagnostics livi-link carries: what the management socket says about
        // the controller, and whether the tunnel could claim it.
        "bt-mgmt"                      => exit_rc(livi_iapd::mgmt::probe()),
        "bt-probe"                     => exit_rc(livi_btd::probe()),
        "livi-btd"       | "btd"       => btd::run(rest),
        "livi-iapd"      | "iapd"      => iapd::run(rest),
        // Unified web UI.
        "livi-httpd"     | "httpd"     => livi_web::run(livi_web::WebCaps {
            model: "V821B + AIC8800D80".into(),
            target: "v821b_aic8800d80".into(),
            port: 80,
            wifi_iface: "wlan0".into(),
            bridge: Some("br0".into()),
            host_iface: "usb0".into(),
            bt: "hci0".into(),
            led: true,
            flash: livi_web::Flash { mtd: true, ..Default::default() },
            update_conf: "/tmp/livi/update.conf".into(),
        }),
        "livi-ledd"      | "ledd"      => ledd::run(rest),
        "livi-netd"      | "netd"      => netd::run(rest),
        "livi-tinyshell" | "tinyshell" => tinyshell::run(rest),
        "livi-config"    | "config"    => config::run(rest),
        "livi-mfid"      | "mfid"      => mfid::run(rest),
        "livi-wifid"     | "wifid"     => wifid::run(rest),
        _ => {
            eprintln!("livid: unknown command '{}'", cmd);
            2
        }
    };
    ExitCode::from(rc.clamp(0, 255) as u8)
}

fn exit_rc(code: ExitCode) -> i32 {
    if code == ExitCode::SUCCESS { 0 } else { 1 }
}
