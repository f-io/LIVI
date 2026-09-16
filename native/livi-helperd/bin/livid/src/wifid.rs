use std::net::TcpListener;
use std::process::Command;

use livi_wifi::server::{Ap, PORT, serve};

const BASE: &str = "/tmp/livi/hostapd.conf.saved";
const LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const LOG: &str = "/tmp/livi/hostapd.log";

pub fn run(_args: Vec<String>) -> i32 {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wifid] bind :{PORT}: {e}");
            return 1;
        }
    };
    println!("[wifid] listening on :{PORT}");
    let mut ap = Ap::new(BASE, LIVE, LOG).with_on_save(Box::new(|| {
        let _ = Command::new("/usr/bin/livid")
            .arg("config").arg("save")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }));
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        serve(&mut stream, &mut ap);
    }
    0
}
