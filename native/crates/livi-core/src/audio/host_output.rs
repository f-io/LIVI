// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(feature = "mutter")]
use std::collections::HashMap;
#[cfg(feature = "mutter")]
use std::time::Duration;

#[cfg(feature = "mutter")]
use zbus::OwnedVariant;

#[cfg(feature = "mutter")]
use livi_core_proto::state::Display;

use crate::hub::Hub;

#[cfg(feature = "mutter")]
const TOOL_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(feature = "mutter")]
const MUTTER: &str = "org.gnome.Mutter.DisplayConfig";
#[cfg(feature = "mutter")]
const MUTTER_PATH: &str = "/org/gnome/Mutter/DisplayConfig";

#[cfg(feature = "mutter")]
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

#[cfg(feature = "mutter")]
type Props = HashMap<String, OwnedVariant>;
#[cfg(feature = "mutter")]
type MutterMode = (String, i32, i32, f64, f64, Vec<f64>, Props);
#[cfg(feature = "mutter")]
type MutterMonitor = ((String, String, String, String), Vec<MutterMode>, Props);
#[cfg(feature = "mutter")]
type MutterLogical = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<(String, String, String, String)>,
    Props,
);
#[cfg(feature = "mutter")]
type MutterState = (u32, Vec<MutterMonitor>, Vec<MutterLogical>, Props);

#[cfg(feature = "mutter")]
fn mutter_displays_in(monitors: &[MutterMonitor]) -> Vec<Display> {
    monitors
        .iter()
        .map(|((connector, _, product, _), modes, _)| {
            let sizes = modes.iter().filter_map(|mode| {
                Some((u32::try_from(mode.1).ok()?, u32::try_from(mode.2).ok()?))
            });
            let flagged = |name: &str| {
                modes.iter().find(|mode| {
                    mode.6
                        .get(name)
                        .and_then(|v| bool::try_from(v).ok())
                        .unwrap_or(false)
                })
            };
            let current = flagged("is-current");
            Display {
                connector: connector.clone(),
                panel: product.clone(),
                current: current.map(|m| format!("{}x{}@{}", m.1, m.2, m.3.round() as u32)),
                panel_default: flagged("is-preferred").map(|m| format!("{}x{}", m.1, m.2)),
                modes: largest_first(sizes),
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(feature = "mutter")]
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

pub async fn prepare(hub: &Hub) {
    if !cfg!(target_os = "linux") {
        return;
    }
    #[cfg(feature = "mutter")]
    {
        let displays = mutter_displays().await;
        if !displays.is_empty() {
            hub.update(|s| s.system.displays = displays);
        }
    }
    #[cfg(not(feature = "mutter"))]
    let _ = hub;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "mutter")]
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
}
