// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum Feed {
    Connected,
    Event(Value),
}

#[derive(Debug, Clone)]
pub struct LineSock {
    path: PathBuf,
    label: &'static str,
}

impl LineSock {
    pub fn new(path: impl Into<PathBuf>, label: &'static str) -> Self {
        Self {
            path: path.into(),
            label,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn request(&self, line: &str, within: Duration) -> Result<Value, String> {
        let label = self.label;
        let exchange = async {
            let mut sock = UnixStream::connect(&self.path)
                .await
                .map_err(|e| format!("{label} error: {e}"))?;
            sock.write_all(format!("{line}\n").as_bytes())
                .await
                .map_err(|e| format!("{label} error: {e}"))?;
            let mut answer = String::new();
            BufReader::new(sock)
                .read_line(&mut answer)
                .await
                .map_err(|e| format!("{label} error: {e}"))?;
            if !answer.ends_with('\n') {
                return Err(format!("{label} closed without response"));
            }
            let json = answer.trim_end();
            serde_json::from_str(json).map_err(|e| format!("{label} bad json: {json} ({e})"))
        };
        tokio::time::timeout(within, exchange)
            .await
            .map_err(|_| format!("{label} timeout after {}ms", within.as_millis()))?
    }

    pub fn subscribe(
        &self,
        retry: Duration,
        keep: fn(&Value) -> bool,
    ) -> mpsc::UnboundedReceiver<Feed> {
        let (tx, rx) = mpsc::unbounded_channel();
        let path = self.path.clone();
        tokio::spawn(async move {
            while !tx.is_closed() {
                if let Ok(mut sock) = UnixStream::connect(&path).await
                    && sock.write_all(b"subscribe\n").await.is_ok()
                {
                    let _ = tx.send(Feed::Connected);
                    let mut lines = BufReader::new(sock).lines();
                    loop {
                        let line = tokio::select! {
                            line = lines.next_line() => line,
                            () = tx.closed() => return,
                        };
                        let Ok(Some(line)) = line else { break };
                        if let Ok(event @ Value::Object(_)) = serde_json::from_str(line.trim())
                            && keep(&event)
                        {
                            let _ = tx.send(Feed::Event(event));
                        }
                    }
                }
                tokio::select! {
                    () = tokio::time::sleep(retry) => {}
                    () = tx.closed() => return,
                }
            }
        });
        rx
    }
}
