// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::Navigation;
use serde_json::Value;

const ROUNDABOUT_EXIT_FIRST: u32 = 28;
const ROUNDABOUT_EXIT_LAST: u32 = 46;
const DRIVING_SIDE_RIGHT: u32 = 0;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Nav {
    pub active: bool,
    pub icon: &'static str,
    pub roundabout_exit: Option<u32>,
    pub image: Option<String>,
    pub distance: String,
    pub maneuver: String,
    pub road: String,
    pub after_road: String,
    pub time_left: String,
    pub destination_distance: String,
    pub destination: String,
    pub app: String,
}

pub fn icon(kind: Option<u32>, turn_side: Option<u32>) -> &'static str {
    let right = turn_side == Some(DRIVING_SIDE_RIGHT);
    match kind {
        None => "NavigationOutlined",
        Some(ROUNDABOUT_EXIT_FIRST..=ROUNDABOUT_EXIT_LAST) => "RoundaboutRight",
        Some(0 | 3 | 5 | 11) => "Straight",
        Some(1) => "TurnLeft",
        Some(2) => "TurnRight",
        Some(4 | 18 | 26) => {
            if right {
                "UTurnRight"
            } else {
                "UTurnLeft"
            }
        }
        Some(6 | 7 | 19) => "RoundaboutRight",
        Some(8 | 22 | 23) => "ExitToApp",
        Some(9) => "Merge",
        Some(10 | 12 | 24 | 25 | 27) => "Flag",
        Some(13 | 52) => "ForkLeft",
        Some(14 | 53) => "ForkRight",
        Some(15..=17) => "DirectionsBoat",
        Some(20) => "SubdirectoryArrowLeft",
        Some(21) => "SubdirectoryArrowRight",
        Some(47) => "TurnSharpLeft",
        Some(48) => "TurnSharpRight",
        Some(49) => "TurnSlightLeft",
        Some(50) => "TurnSlightRight",
        Some(51) => "SwapHoriz",
        Some(_) => "NavigationOutlined",
    }
}

pub fn nav(state: &Value) -> Nav {
    let n: Navigation = serde_json::from_value(state["navigation"].clone()).unwrap_or_default();
    let text = |s: &Option<String>| s.clone().unwrap_or_default();
    Nav {
        active: n.active == Some(true),
        icon: icon(n.maneuver_type, n.turn_side),
        roundabout_exit: n
            .maneuver_type
            .filter(|t| (ROUNDABOUT_EXIT_FIRST..=ROUNDABOUT_EXIT_LAST).contains(t))
            .map(|t| t - ROUNDABOUT_EXIT_FIRST + 1),
        image: n.image.clone().filter(|i| !i.is_empty()),
        distance: text(&n.maneuver_distance_text),
        maneuver: text(&n.maneuver_text),
        road: text(&n.road_name),
        after_road: text(&n.after_road_name),
        time_left: text(&n.time_left_text),
        destination_distance: text(&n.destination_distance_text),
        destination: text(&n.destination_name),
        app: text(&n.app_name),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maneuvers_pick_their_icon() {
        assert_eq!(icon(Some(1), None), "TurnLeft");
        assert_eq!(icon(Some(4), Some(0)), "UTurnRight");
        assert_eq!(icon(Some(4), Some(1)), "UTurnLeft");
        assert_eq!(icon(None, None), "NavigationOutlined");
    }

    #[test]
    fn a_roundabout_exit_carries_its_number() {
        let n = nav(&json!({ "navigation": { "active": true, "maneuverType": 30 } }));
        assert_eq!(n.roundabout_exit, Some(3));
        assert_eq!(n.icon, "RoundaboutRight");
        assert!(n.active);
    }
}
