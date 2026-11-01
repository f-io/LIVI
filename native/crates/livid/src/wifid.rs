// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use livi_net::port::CONTROL;
use livi_wifid::server::{Ap, serve};

const BASE: &str = "/tmp/livi/hostapd.conf.saved";
const LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const LOG: &str = "/tmp/livi/hostapd.log";
const IDLE: Duration = Duration::from_secs(5);

pub fn run(_args: Vec<String>) -> i32 {
    let listener = match TcpListener::bind(("0.0.0.0", CONTROL)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wifid] bind :{CONTROL}: {e}");
            return 1;
        }
    };
    println!("[wifid] listening on :{CONTROL}");
    let standards = crate::wifi_standards(crate::board().1);
    let ap = Ap::new(BASE, LIVE, LOG)
        .with_standards(standards)
        .with_on_save(Box::new(|| {
            let _ = Command::new("/usr/bin/livid")
                .arg("config")
                .arg("save")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }));
    let ap = Arc::new(Mutex::new(ap));
    for mut stream in livi_net::bridge::from_usb(&listener) {
        let ap = ap.clone();
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(IDLE));
            serve(&mut stream, &ap);
        });
    }
    0
}
