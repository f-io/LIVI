// macOS: no BlueZ and no MFi coprocessor here, so the helper serves Android Auto
// and the dongle over USB and the event socket the main process subscribes to.

use std::process::ExitCode;

use livi_runtime::livi_sock::Broadcaster;

pub fn run() -> ExitCode {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[helperd] runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(async {
        let aa_events = Broadcaster::default();
        let deps = livi_runtime::aa_sock::AaSockDeps {
            adapter: String::new(),
            wifi_iface: String::new(),
            set_wired_phones: Box::new(|_| {}),
            events: aa_events.clone(),
            set_playback_status: Box::new(|_| {}),
            set_sco_sink: Box::new(|_| {}),
        };
        tokio::spawn(async move {
            if let Err(e) = livi_runtime::aa_sock::serve(None, deps).await {
                eprintln!("[aa-sock] ended: {e}");
            }
        });
        let events = aa_events.clone();
        tokio::spawn(livi_aa::usb::run(move |socket, peer, serial| {
            events.push_json(format!(
                "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"usb\",\"serial\":\"{serial}\"}}"
            ));
        }));
        println!("[helperd] Android Auto USB watcher started");
        let events = aa_events.clone();
        tokio::spawn(livi_dongle::run(move |socket, a| {
            events.push_json(
                serde_json::json!({
                    "event": "dongle-session", "socket": socket, "serial": a.serial,
                    "product": a.product, "version": a.version, "name": a.name
                })
                .to_string(),
            );
        }));
        println!("[helperd] dongle watcher started");
        let _ = tokio::signal::ctrl_c().await;
    });
    ExitCode::SUCCESS
}
