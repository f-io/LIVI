// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use zbus::OwnedVariant;

use livi_core_proto::state::Display;

use crate::hub::Hub;
use crate::system::privileged;

const TOOL_TIMEOUT: Duration = Duration::from_secs(3);
const PIN_TIMEOUT: Duration = Duration::from_secs(5);
const VIDEO_MODE_HELPER: &str = "/usr/local/lib/livi/livi-video-mode.sh";
const MUTTER: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_PATH: &str = "/org/gnome/Mutter/DisplayConfig";

fn largest_first(sizes: impl Iterator<Item = (u32, u32)>) -> Vec<String> {
    let mut modes: Vec<(u32, u32)> = Vec::new();
    for size in sizes {
        if !modes.contains(&size) {
            modes.push(size);
        }
    }
    modes.sort_by_key(|&(w, h)| std::cmp::Reverse(u64::from(w) * u64::from(h)));
    modes.into_iter().map(|(w, h)| format!("{w}x{h}")).collect()
}

type Props = HashMap<String, OwnedVariant>;
type MutterMode = (String, i32, i32, f64, f64, Vec<f64>, Props);
type MutterMonitor = ((String, String, String, String), Vec<MutterMode>, Props);
type MutterLogical = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<(String, String, String, String)>,
    Props,
);
type MutterState = (u32, Vec<MutterMonitor>, Vec<MutterLogical>, Props);

fn mutter_displays_in(monitors: &[MutterMonitor]) -> Vec<Display> {
    monitors
        .iter()
        .map(|((connector, _, product, _), modes, _)| {
            let sizes = modes.iter().filter_map(|mode| {
                Some((u32::try_from(mode.1).ok()?, u32::try_from(mode.2).ok()?))
            });
            let current = modes.iter().find(|mode| {
                mode.6
                    .get("is-current")
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(false)
            });
            Display {
                connector: connector.clone(),
                panel: product.clone(),
                current: current.map(|m| format!("{}x{}@{}", m.1, m.2, m.3.round() as u32)),
                modes: largest_first(sizes),
                ..Default::default()
            }
        })
        .collect()
}

async fn mutter_displays() -> Vec<Display> {
    let state = async {
        let bus = zbus::Connection::session().await.ok()?;
        let reply = bus
            .call_method(
                Some(MUTTER),
                MUTTER_PATH,
                Some(MUTTER),
                "GetCurrentState",
                &(),
            )
            .await
            .ok()?;
        reply.body().deserialize::<MutterState>().ok()
    };
    match tokio::time::timeout(TOOL_TIMEOUT, state).await {
        Ok(Some((_, monitors, _, _))) => mutter_displays_in(&monitors),
        _ => Vec::new(),
    }
}

fn pin_arg(connector: &str, current: &str) -> String {
    format!("{connector}:{current}")
}

pub async fn pin(connector: &str, current: &str) {
    if !Path::new(VIDEO_MODE_HELPER).exists() {
        return;
    }
    let pin = pin_arg(connector, current);
    if privileged::sudo_within(&[VIDEO_MODE_HELPER, &pin], PIN_TIMEOUT).await {
        println!("[display] cmdline video pin → {pin}");
    } else {
        println!("[display] could not pin {pin} in the cmdline");
    }
}

pub async fn prepare(hub: &Hub) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let displays = mutter_displays().await;
    if !displays.is_empty() {
        hub.update(|s| s.system.displays = displays);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_gives_its_modes_as_sizes_of_every_monitor() {
        let mode = |w: i32, h: i32, hz: f64| {
            (
                format!("{w}x{h}@{hz:.3}"),
                w,
                h,
                hz,
                1.0,
                vec![1.0],
                Props::new(),
            )
        };
        let id = |c: &str| (c.to_string(), String::new(), String::new(), String::new());
        let monitors = vec![
            (
                id("DP-4"),
                vec![mode(3840, 2160, 60.0), mode(3840, 2160, 59.982)],
                Props::new(),
            ),
            (
                id("HDMI-1"),
                vec![mode(1280, 720, 60.0), mode(-1, 480, 60.0)],
                Props::new(),
            ),
        ];
        let displays = mutter_displays_in(&monitors);
        assert_eq!(displays[0].connector, "DP-4");
        assert_eq!(displays[0].modes, ["3840x2160"]);
        assert_eq!(displays[1].modes, ["1280x720"]);
        assert_eq!(displays[0].current, None);
        assert!(mutter_displays_in(&[]).is_empty());
    }

    #[test]
    fn the_pin_names_connector_mode_and_rate() {
        assert_eq!(pin_arg("HDMI-A-1", "1280x720@60"), "HDMI-A-1:1280x720@60");
    }
}
