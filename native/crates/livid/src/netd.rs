// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn run(args: Vec<String>) -> i32 {
    crate::exit_rc(livid_main(&args))
}

use std::net::Ipv4Addr;
use std::process::ExitCode;
use std::thread;

fn livid_main(args: &[String]) -> ExitCode {
    let iface = args.first().cloned().unwrap_or_else(|| "usb0".into());
    let server_ip: Ipv4Addr = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(Ipv4Addr::new(10, 10, 10, 1));
    let pool_start: u8 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let pool_end: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(199);
    let hostname: String = args.get(4).cloned().unwrap_or_else(|| "livi-link".into());

    eprintln!(
        "[livi-netd] iface={iface} server={server_ip} pool=10.10.10.{pool_start}..{pool_end} host={hostname}.local"
    );

    let iface_c = iface.clone();
    thread::spawn(move || {
        if let Err(e) = livi_net::dhcp::serve(&iface_c, server_ip, pool_start, pool_end) {
            eprintln!("[livi-netd] dhcp: {e}");
        }
    });

    livi_mdns::daemon::run(&[hostname, iface])
}
