// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::Path;

use livi_core_proto::helper_requests::AaRequest;
use tokio::net::UnixStream;

use crate::shared_sock::{bind, err_json, ok_json, read_line, reply, run_subscriber};

type SetWiredPhones = Box<dyn Fn(Vec<String>) + Send + Sync>;
type SetScoSink = Box<dyn Fn(Option<(String, u32)>) + Send + Sync>;

pub struct AaSockDeps {
    pub set_wired_phones: SetWiredPhones,
    pub events: crate::livi_sock::Broadcaster,
    pub set_sco_sink: SetScoSink,
    pub restart_usb: Box<dyn Fn(&str) -> usize + Send + Sync>,
}

pub async fn serve(path: &Path, deps: AaSockDeps) -> io::Result<()> {
    let listener = bind(path, "aa-sock")?;
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
    let json = match AaRequest::parse(&line) {
        Ok(AaRequest::Subscribe) => {
            return run_subscriber(stream, deps.events.clone(), "aa-sock").await;
        }
        Ok(AaRequest::WiredPhones(ids)) => {
            (deps.set_wired_phones)(ids);
            ok_json()
        }
        Ok(AaRequest::ScoSink(target)) => {
            (deps.set_sco_sink)(target);
            ok_json()
        }
        Ok(AaRequest::RestartUsb(serial)) => {
            let n = (deps.restart_usb)(serial.as_deref().unwrap_or(""));
            println!("[aa-sock] restart-usb: {n} session(s) end for a fresh start");
            format!("{{\"ok\":true,\"count\":{n}}}")
        }
        Err(e) => err_json(&e),
    };
    reply(&mut stream, &json).await
}
