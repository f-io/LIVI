// CPC200 wifid: binds :5001 and drives hostapd. Shared logic lives in
// livi_wifi::server. This wrapper only wires the paths and accept loop.

use std::net::TcpListener;
use std::process::ExitCode;

use livi_wifi::server::{Ap, PORT, ap_name_from, serve};

const BASE: &str = "/etc/hostapd.conf";
const LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const LOG: &str = "/tmp/livi/hostapd.log";

pub fn run() -> ExitCode {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wifid] bind :{PORT}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("[wifid] listening on :{PORT}");
    let mut ap = Ap::new(BASE, LIVE, LOG);
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        serve(&mut stream, &mut ap);
    }
    ExitCode::SUCCESS
}

pub fn ap_name() -> Option<String> {
    ap_name_from(
        std::path::Path::new(BASE),
        &[std::path::Path::new(LIVE[0]), std::path::Path::new(LIVE[1])],
    )
}
