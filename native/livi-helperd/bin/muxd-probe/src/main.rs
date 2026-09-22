use std::process::ExitCode;

#[cfg(target_os = "linux")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use iap2_usbmux::{find_iphones, MuxRegistry};

    let keep = std::env::args().any(|a| a == "--keep");

    let phones = find_iphones();
    if phones.is_empty() {
        return Err("no iPhone (VID 05ac) found on USB".into());
    }
    for p in &phones {
        println!(
            "[muxd-probe] iphone serial={} nConfigs={} config={:?}",
            p.serial, p.num_configs, p.config_value
        );
    }

    let registry = MuxRegistry::default();
    let (added, _) = registry.sync();
    println!("[muxd-probe] brought up {} phone(s): {:?}", added.len(), short(&added));

    // All phones talk to lockdown at the same time — proves the stack is per-device.
    let handles: Vec<_> = registry
        .serials()
        .into_iter()
        .map(|serial| {
            let dev = registry.get(&serial).expect("device just registered");
            std::thread::spawn(move || {
                let tag = &serial[..8.min(serial.len())];
                println!("[muxd-probe] {tag}: socket {}", dev.socket_path);
                match query_lockdown(&dev) {
                    Ok((n, typ)) => println!("[muxd-probe] {tag}: lockdown {n} bytes, Type={typ}"),
                    Err(e) => println!("[muxd-probe] {tag}: lockdown failed: {e}"),
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }

    if std::env::args().any(|a| a == "--carkit") {
        probe_carkit(&registry);
    }

    if keep {
        println!("[muxd-probe] leaving phones in config 6 (--keep)");
        std::mem::forget(registry);
    } else {
        println!("[muxd-probe] restoring default config");
    }
    Ok(())
}

// Full wired path: lockdown session over the pair record, then com.apple.carkit.service,
// whose TLS stream carries iAP2. All phones at once.
#[cfg(target_os = "linux")]
fn probe_carkit(registry: &iap2_usbmux::MuxRegistry) {
    use iap2_wired::{open_carkit, pair_record_path};

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            println!("[muxd-probe] runtime: {e}");
            return;
        }
    };

    rt.block_on(async {
        let mut tasks = Vec::new();
        for serial in registry.serials() {
            let Some(dev) = registry.get(&serial) else { continue };
            tasks.push(tokio::spawn(async move {
                let tag = serial[..8.min(serial.len())].to_string();
                match pair_record_path(&serial) {
                    Some(p) => println!("[muxd-probe] {tag}: pair record {}", p.display()),
                    None => println!("[muxd-probe] {tag}: no pair record yet — will pair (confirm on phone)"),
                }
                match open_carkit(&dev).await {
                    Ok(mut ch) => {
                        println!("[muxd-probe] {tag}: carkit TLS channel up (iAP2)");
                        match tokio::time::timeout(std::time::Duration::from_secs(5), ch.recv(64)).await {
                            Ok(Ok(data)) => println!(
                                "[muxd-probe] {tag}: first iAP2 bytes ({}) {:02x?}",
                                data.len(),
                                &data[..data.len().min(8)]
                            ),
                            Ok(Err(e)) => println!("[muxd-probe] {tag}: carkit read failed: {e}"),
                            Err(_) => println!("[muxd-probe] {tag}: no iAP2 bytes yet (phone waits for us)"),
                        }
                    }
                    Err(e) => println!("[muxd-probe] {tag}: carkit failed: {e}"),
                }
            }));
        }
        for t in tasks {
            let _ = t.await;
        }
    });
}

#[cfg(target_os = "linux")]
fn short(serials: &[String]) -> Vec<String> {
    serials.iter().map(|s| s[..8.min(s.len())].to_string()).collect()
}

#[cfg(target_os = "linux")]
fn query_lockdown(dev: &iap2_usbmux::MuxDevice) -> Result<(usize, String), String> {
    use iap2_usbmux::LOCKDOWN_PORT;
    use std::time::Duration;

    let conn = dev.host.connect(LOCKDOWN_PORT)?;
    let body = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Request</key><string>QueryType</string></dict></plist>";
    let mut msg = (body.len() as u32).to_be_bytes().to_vec();
    msg.extend_from_slice(body);
    conn.send(&msg);

    let mut buf: Vec<u8> = Vec::new();
    let mut expect: Option<usize> = None;
    loop {
        match conn.recv(Duration::from_secs(5)) {
            Some(data) if !data.is_empty() => {
                buf.extend_from_slice(&data);
                if expect.is_none() && buf.len() >= 4 {
                    expect = Some(u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize);
                }
                if let Some(n) = expect
                    && buf.len() >= 4 + n {
                        let text = String::from_utf8_lossy(&buf[4..4 + n]);
                        let typ = text
                            .split("<string>")
                            .nth(1)
                            .and_then(|s| s.split("</string>").next())
                            .unwrap_or("?")
                            .to_string();
                        conn.close();
                        return Ok((n, typ));
                    }
            }
            Some(_) => {
                conn.close();
                return Err("closed without full reply".into());
            }
            None => {
                conn.close();
                return Err(format!("timed out (got {} bytes)", buf.len()));
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("muxd-probe needs USB; build and run it on the Pi".into())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[muxd-probe] error: {e}");
            ExitCode::FAILURE
        }
    }
}
