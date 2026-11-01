// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::state::AudioDevice;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::devices;
use crate::hub::Hub;
use crate::resources::gst_env;

const MONITOR: &str = "gst-device-monitor-1.0";
const LIST_TIMEOUT: Duration = Duration::from_secs(4);
const PAIRED_TIMEOUT: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_millis(250);
const RESTART_DELAY: Duration = Duration::from_secs(2);
const BT_MAJOR_AUDIO: u32 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Sink,
    Source,
}

impl Kind {
    fn class(self) -> &'static str {
        match self {
            Kind::Sink => "Audio/Sink",
            Kind::Source => "Audio/Source",
        }
    }
}

struct Paired {
    mac: String,
    name: String,
}

fn prop<'a>(block: &[&'a str], key: &str) -> Option<&'a str> {
    block.iter().find_map(|line| {
        let rest = line.trim_start().strip_prefix(key)?.trim_start();
        let value = rest.strip_prefix([':', '='])?.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        (!value.is_empty()).then_some(value)
    })
}

fn launch_id(block: &[&str]) -> Option<String> {
    let line = block
        .iter()
        .find_map(|l| l.find("gst-launch-1.0").map(|at| &l[at..]))?;
    let bytes = line.as_bytes();
    let starts = line
        .match_indices("device=")
        .chain(line.match_indices("unique-id="));
    let (at, key) = starts
        .filter(|(at, _)| {
            *at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_')
        })
        .min_by_key(|(at, _)| *at)?;
    let value = &line[at + key.len()..];
    let id = match value.chars().next()? {
        q @ ('"' | '\'') => value[1..].split(q).next()?,
        _ => value
            .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .next()?,
    };
    (!id.is_empty()).then(|| id.to_string())
}

fn parse(stdout: &str, kind: Kind) -> Vec<AudioDevice> {
    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in stdout.lines() {
        if line.trim() == "Device found:" {
            blocks.push(Vec::new());
        } else if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
    }
    let mut devices: Vec<AudioDevice> = Vec::new();
    for block in &blocks {
        if !prop(block, "class").is_some_and(|c| c.contains(kind.class())) {
            continue;
        }
        if kind == Kind::Source && prop(block, "device.class") == Some("monitor") {
            continue;
        }
        let id = launch_id(block).or_else(|| {
            [
                "unique-id",
                "device.name",
                "node.name",
                "alsa.card_name",
                "name",
            ]
            .iter()
            .find_map(|k| prop(block, k))
            .map(str::to_string)
        });
        let Some(id) = id else { continue };
        let name = [
            "name",
            "device.description",
            "node.description",
            "alsa.long_card_name",
        ]
        .iter()
        .find_map(|k| prop(block, k))
        .map(str::to_string)
        .unwrap_or_else(|| id.clone());
        let is_default = prop(block, "default").is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || matches!(prop(block, "is-default"), Some("true" | "true (gboolean)"))
            || prop(block, "node.is-default") == Some("true");
        if !devices.iter().any(|d| d.id == id) {
            devices.push(AudioDevice {
                id,
                name,
                is_default,
                offline: false,
            });
        }
    }
    devices
}

fn bluez_mac(id: &str) -> Option<String> {
    let rest = [
        "bluez_output.",
        "bluez_input.",
        "bluez_sink.",
        "bluez_source.",
    ]
    .iter()
    .find_map(|p| id.strip_prefix(p))?;
    let mac = rest.get(..17)?;
    mac.chars()
        .all(|c| c.is_ascii_hexdigit() || c == '_' || c == ':')
        .then(|| mac.replace('_', ":").to_uppercase())
}

fn with_paired(devices: Vec<AudioDevice>, paired: &[Paired], kind: Kind) -> Vec<AudioDevice> {
    let mut macs: Vec<String> = Vec::new();
    let mut live: Vec<AudioDevice> = Vec::new();
    for d in devices {
        if let Some(mac) = bluez_mac(&d.id) {
            if macs.contains(&mac) {
                continue;
            }
            macs.push(mac);
        }
        live.push(d);
    }
    let names: Vec<String> = live.iter().map(|d| d.name.trim().to_lowercase()).collect();
    let offline: Vec<AudioDevice> = paired
        .iter()
        .filter(|p| !macs.contains(&p.mac.to_uppercase()))
        .filter(|p| !names.contains(&p.name.trim().to_lowercase()))
        .map(|p| AudioDevice {
            id: match kind {
                Kind::Sink => format!("bluez_output.{}.0", p.mac.to_uppercase().replace(':', "_")),
                Kind::Source => format!("bluez_input.{}", p.mac.to_uppercase()),
            },
            name: p.name.clone(),
            is_default: false,
            offline: true,
        })
        .collect();
    live.into_iter().chain(offline).collect()
}

fn monitor(gst: Option<&Path>) -> Command {
    let mut cmd =
        Command::new(gst.map_or_else(|| PathBuf::from(MONITOR), |r| r.join("bin").join(MONITOR)));
    if let Some(root) = gst {
        cmd.envs(gst_env(root));
    }
    cmd.stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    cmd
}

async fn list(gst: Option<&Path>, kind: Kind) -> Vec<AudioDevice> {
    let out = monitor(gst).arg(kind.class()).output();
    match tokio::time::timeout(LIST_TIMEOUT, out).await {
        Ok(Ok(out)) if out.status.success() => parse(&String::from_utf8_lossy(&out.stdout), kind),
        _ => Vec::new(),
    }
}

async fn paired_audio() -> Vec<Paired> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let Ok(answer) = devices::shared("list_paired".into(), PAIRED_TIMEOUT).await else {
        return Vec::new();
    };
    let Some(list) = answer.get("devices").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter(|d| {
            let class = d.get("class").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
            class > 0 && (class >> 8) & 0x1f == BT_MAJOR_AUDIO
        })
        .filter_map(|d| {
            let mac = d.get("mac")?.as_str()?.to_string();
            let name = d
                .get("name")
                .and_then(|n| n.as_str())
                .filter(|n| !n.is_empty());
            Some(Paired {
                name: name.unwrap_or(&mac).to_string(),
                mac,
            })
        })
        .collect()
}

async fn refresh(hub: &Hub, gst: Option<&Path>) {
    let paired = paired_audio().await;
    let sinks = with_paired(list(gst, Kind::Sink).await, &paired, Kind::Sink);
    let sources = with_paired(list(gst, Kind::Source).await, &paired, Kind::Source);
    hub.update(|s| {
        s.system.audio_sinks = sinks;
        s.system.audio_sources = sources;
    });
}

fn topology_changed(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("Device") else {
        return false;
    };
    let Some(word) = rest.strip_suffix(':') else {
        return false;
    };
    rest.starts_with(char::is_whitespace) && {
        let word = word.trim();
        !word.is_empty() && !word.contains(char::is_whitespace)
    }
}

async fn watch(hub: &Hub, gst: Option<&Path>) -> bool {
    let mut cmd = monitor(gst);
    cmd.args(["-f", Kind::Sink.class(), Kind::Source.class()])
        .stdout(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let Some(out) = child.stdout.take() else {
        return true;
    };
    let mut lines = BufReader::new(out).lines();
    let mut pending = false;
    loop {
        let next = if pending {
            match tokio::time::timeout(SETTLE, lines.next_line()).await {
                Ok(next) => next,
                Err(_) => {
                    pending = false;
                    refresh(hub, gst).await;
                    continue;
                }
            }
        } else {
            lines.next_line().await
        };
        match next {
            Ok(Some(line)) => pending |= topology_changed(&line),
            _ => break,
        }
    }
    if pending {
        refresh(hub, gst).await;
    }
    true
}

pub async fn follow(hub: Arc<Hub>, gst: Option<PathBuf>) {
    refresh(&hub, gst.as_deref()).await;
    while watch(&hub, gst.as_deref()).await {
        tokio::time::sleep(RESTART_DELAY).await;
    }
    println!("[audio] {MONITOR} does not start, the device lists stay as they are");
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = "Probing devices...\n\n\nDevice found:\n\n\tname  : Built-in Audio Analog Stereo\n\
        \tclass : Audio/Sink\n\tcaps  : audio/x-raw, format=(string)S16LE\n\tproperties:\n\
        \t\tdevice.class = sound\n\t\tnode.name = alsa_output.pci.analog-stereo\n\
        \t\tis-default = true\n\
        \tgst-launch-1.0 ... ! pulsesink device=alsa_output.pci.analog-stereo\n\n\
        Device found:\n\n\tname  : Headset\n\tclass : Audio/Sink\n\
        \tgst-launch-1.0 ... ! 'pulsesink device=bluez_output.AA_BB_CC_DD_EE_FF.1'\n\n\
        Device found:\n\n\tname  : Headset\n\tclass : Audio/Sink\n\
        \tgst-launch-1.0 ... ! pulsesink device=bluez_output.AA_BB_CC_DD_EE_FF.2\n\n\
        Device found:\n\n\tname  : Mic\n\tclass : Audio/Source\n";

    fn ids(devices: &[AudioDevice]) -> Vec<&str> {
        devices.iter().map(|d| d.id.as_str()).collect()
    }

    #[test]
    fn devices_come_from_the_monitor_listing() {
        let sinks = parse(SINKS, Kind::Sink);
        assert_eq!(
            ids(&sinks),
            [
                "alsa_output.pci.analog-stereo",
                "bluez_output.AA_BB_CC_DD_EE_FF.1",
                "bluez_output.AA_BB_CC_DD_EE_FF.2"
            ]
        );
        assert_eq!(sinks[0].name, "Built-in Audio Analog Stereo");
        assert!(sinks[0].is_default && !sinks[1].is_default);
        assert_eq!(ids(&parse(SINKS, Kind::Source)), ["Mic"]);
        assert!(parse("", Kind::Sink).is_empty());
    }

    #[test]
    fn the_id_falls_back_to_the_properties_and_monitors_are_left_out() {
        let mac = "Device found:\n\tname : MacBook Speakers\n\tclass : Audio/Sink\n\
            \tgst-launch-1.0 ... ! osxaudiosink unique-id=\"BuiltInSpeakerDevice\"\n";
        assert_eq!(ids(&parse(mac, Kind::Sink)), ["BuiltInSpeakerDevice"]);
        let props = "Device found:\n\tclass : Audio/Source\n\t\tnode.name = in.usb\n\
            \t\tnode.is-default = true\n";
        let found = parse(props, Kind::Source);
        assert_eq!(
            (found[0].id.as_str(), found[0].name.as_str()),
            ("in.usb", "in.usb")
        );
        assert!(found[0].is_default);
        let monitor = "Device found:\n\tname : Monitor of X\n\tclass : Audio/Source\n\
            \t\tdevice.class = monitor\n";
        assert!(parse(monitor, Kind::Source).is_empty());
        assert_eq!(
            launch_id(&["gst-launch-1.0 ! sink audio-device=x"]).as_deref(),
            Some("x")
        );
        assert_eq!(launch_id(&["gst-launch-1.0 ! sink my_device=x"]), None);
    }

    #[test]
    fn bluetooth_shows_once_per_device_and_paired_ones_wait_offline() {
        let paired = [
            Paired {
                mac: "AA:BB:CC:DD:EE:FF".into(),
                name: "Headset".into(),
            },
            Paired {
                mac: "11:22:33:44:55:66".into(),
                name: "Car Kit".into(),
            },
            Paired {
                mac: "77:88:99:AA:BB:CC".into(),
                name: "built-in audio analog stereo".into(),
            },
        ];
        let sinks = with_paired(parse(SINKS, Kind::Sink), &paired, Kind::Sink);
        assert_eq!(
            ids(&sinks),
            [
                "alsa_output.pci.analog-stereo",
                "bluez_output.AA_BB_CC_DD_EE_FF.1",
                "bluez_output.11_22_33_44_55_66.0"
            ]
        );
        assert!(sinks[2].offline && !sinks[1].offline);
        let sources = with_paired(Vec::new(), &paired[1..2], Kind::Source);
        assert_eq!(ids(&sources), ["bluez_input.11:22:33:44:55:66"]);
        assert_eq!(
            bluez_mac("bluez_input.aa:bb:cc:dd:ee:ff").as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(bluez_mac("bluez_output.short"), None);
    }

    #[test]
    fn only_device_lines_count_as_a_change() {
        assert!(topology_changed("Device added:"));
        assert!(topology_changed("  Device removed: "));
        assert!(!topology_changed("Device found: something"));
        assert!(!topology_changed("Devices:"));
        assert!(!topology_changed("\tname : Device x:"));
    }
}
