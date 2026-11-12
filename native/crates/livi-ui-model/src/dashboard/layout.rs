// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::State;
use livi_core_proto::theme::{Cluster, DisplayKind, ThemeLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Center {
    None,
    NavMini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub gauges: bool,
    pub map: bool,
    pub center: Center,
    pub strip: bool,
}

const MAP_WITH_GAUGES: Spec = Spec {
    gauges: true,
    map: true,
    center: Center::None,
    strip: false,
};

pub fn spec(layout: &str) -> Spec {
    match layout {
        "livi.strip" => Spec {
            gauges: false,
            strip: true,
            ..MAP_WITH_GAUGES
        },
        "livi.navMini" => Spec {
            gauges: true,
            map: false,
            center: Center::NavMini,
            strip: false,
        },
        "livi.clusterOnly" => Spec {
            gauges: false,
            ..MAP_WITH_GAUGES
        },
        _ => MAP_WITH_GAUGES,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pages {
    pub active: Option<String>,
    pub index: Option<usize>,
    pub count: usize,
    pub ids: Vec<String>,
    pub accent: Option<String>,
}

fn layouts(c: &Cluster) -> Vec<ThemeLayout> {
    c.theme
        .displays
        .iter()
        .find(|d| d.kind == DisplayKind::Cluster)
        .map(|d| d.layouts.clone())
        .unwrap_or_default()
}

pub fn pages(state: &State, dark: bool) -> Pages {
    let c = &state.cluster;
    let all = layouts(c);
    let ids: Vec<String> = c
        .shown
        .iter()
        .filter(|id| all.iter().any(|l| &l.id == *id))
        .cloned()
        .collect();
    let active = all.iter().find(|l| l.id == c.layout).map(|l| l.id.clone());
    let accent = c
        .theme
        .palettes
        .iter()
        .find(|p| p.id == c.palette)
        .map(|p| {
            if dark {
                p.accent_dark.clone()
            } else {
                p.accent_light.clone()
            }
        });
    Pages {
        index: ids.iter().position(|id| Some(id) == active.as_ref()),
        count: ids.len(),
        active,
        ids,
        accent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_layouts_keep_the_gauges_over_the_map() {
        assert_eq!(spec("vendor.x"), MAP_WITH_GAUGES);
        assert!(!spec("livi.clusterOnly").gauges);
        assert_eq!(spec("livi.navMini").center, Center::NavMini);
        let strip = spec("livi.strip");
        assert!(strip.strip && strip.map && !strip.gauges);
    }
}
