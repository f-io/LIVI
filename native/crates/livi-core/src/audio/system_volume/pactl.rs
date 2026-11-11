// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{Fault, OutputEvent};

const PACTL: &str = "pactl";
const DEFAULT_SINK: &str = "@DEFAULT_SINK@";
const NO_SUCH_ENTITY: &str = "No such entity";
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const RESTART_DELAY: Duration = Duration::from_secs(2);

fn parse_volume(out: &str) -> Option<f64> {
    let at = out.find('%')?;
    let digits: String = out[..at]
        .trim_end()
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let pct: f64 = digits.parse().ok()?;
    Some((pct / 100.0).clamp(0.0, 1.0))
}

fn sink_name(device: &str) -> &str {
    if device.is_empty() {
        DEFAULT_SINK
    } else {
        device
    }
}

async fn pactl(args: &[&str]) -> Result<String, Fault> {
    let run = Command::new(PACTL)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output();
    let out = tokio::time::timeout(CALL_TIMEOUT, run)
        .await
        .map_err(|_| Fault::Other("timed out".into()))?
        .map_err(|e| Fault::Other(e.to_string()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if err.contains(NO_SUCH_ENTITY) {
        Fault::Missing
    } else {
        Fault::Other(err)
    })
}

pub async fn get(device: &str) -> Result<f64, Fault> {
    let out = pactl(&["get-sink-volume", sink_name(device)]).await?;
    parse_volume(&out).ok_or_else(|| Fault::Other(format!("no level in {out:?}")))
}

pub async fn set(device: &str, level: f64) -> Result<(), Fault> {
    let pct = format!("{}%", (level.clamp(0.0, 1.0) * 100.0).round() as u32);
    pactl(&["set-sink-volume", sink_name(device), &pct])
        .await
        .map(drop)
}

pub struct Watch(JoinHandle<()>);

impl Drop for Watch {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn watch(tx: mpsc::UnboundedSender<OutputEvent>) -> Watch {
    Watch(tokio::spawn(async move {
        loop {
            let child = Command::new(PACTL)
                .arg("subscribe")
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            if let Ok(mut child) = child
                && let Some(out) = child.stdout.take()
            {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let event = if line.contains("Event 'change' on sink #") {
                        OutputEvent::Changed
                    } else if line.contains("Event 'new' on sink #") {
                        OutputEvent::New
                    } else {
                        continue;
                    };
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
            tokio::time::sleep(RESTART_DELAY).await;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_is_the_first_percentage() {
        assert_eq!(
            parse_volume("Volume: front-left: 42598 /  65% / -11.23 dB"),
            Some(0.65)
        );
        assert_eq!(
            parse_volume("Volume: mono: 98304 / 150% / 10 dB"),
            Some(1.0)
        );
        assert_eq!(parse_volume("Volume: 0%"), Some(0.0));
        assert_eq!(parse_volume("no level here"), None);
        assert_eq!(parse_volume("%"), None);
    }

    #[test]
    fn no_configured_sink_is_the_default_one() {
        assert_eq!(sink_name(""), DEFAULT_SINK);
        assert_eq!(sink_name("alsa_output.usb"), "alsa_output.usb");
    }
}
