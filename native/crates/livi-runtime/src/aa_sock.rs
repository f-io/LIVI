// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;

use tokio::net::UnixStream;

use crate::shared_sock::{bind, err_json, ok_json, read_line, reply, run_subscriber};

pub const SOCK_PATH: &str = "/tmp/aa-bt.sock";

type SetWiredPhones = Box<dyn Fn(Vec<String>) + Send + Sync>;
type SetScoSink = Box<dyn Fn(Option<(String, u32)>) + Send + Sync>;

pub struct AaSockDeps {
    pub set_wired_phones: SetWiredPhones,
    pub events: crate::livi_sock::Broadcaster,
    pub set_sco_sink: SetScoSink,
    pub restart_usb: Box<dyn Fn(&str) -> usize + Send + Sync>,
}

pub async fn serve(deps: AaSockDeps) -> io::Result<()> {
    let listener = bind(SOCK_PATH, "aa-sock")?;
    let deps = std::sync::Arc::new(deps);
    loop {
        let (stream, _) = listener.accept().await?;
        let deps = deps.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, deps).await {
                eprintln!("[aa-sock] connection error: {e}");
            }
        });
    }
}

async fn handle(mut stream: UnixStream, deps: std::sync::Arc<AaSockDeps>) -> io::Result<()> {
    let line = read_line(&mut stream).await?;
    let (verb, arg) = match line.split_once(' ') {
        Some((v, a)) => (v, a.trim()),
        None => (line.as_str(), ""),
    };

    if verb == "subscribe" {
        return run_subscriber(stream, deps.events.clone(), "aa-sock").await;
    }

    let json = match verb {
        "wired-phones" => {
            let ids: Vec<String> =
                serde_json::from_str(if arg.is_empty() { "[]" } else { arg }).unwrap_or_default();
            (deps.set_wired_phones)(ids);
            ok_json()
        }
        "sco-sink" => {
            let target = arg.split_once(' ').and_then(|(feed, id)| {
                id.trim()
                    .parse::<u32>()
                    .ok()
                    .map(|id| (feed.to_owned(), id))
            });
            (deps.set_sco_sink)(target);
            ok_json()
        }
        "restart-usb" => {
            let n = (deps.restart_usb)(arg);
            println!("[aa-sock] restart-usb: {n} session(s) end for a fresh start");
            format!("{{\"ok\":true,\"count\":{n}}}")
        }
        other => err_json(&format!("unknown command: {other}")),
    };
    reply(&mut stream, &json).await
}
