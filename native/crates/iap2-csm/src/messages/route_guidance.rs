// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::{csm_group, csm_message};

csm_message! {
    pub struct StartRouteGuidanceUpdates = 0x5200 {
        0 => display_component_id: [opt u16],
        1 => source_name: [flag],
        2 => source_supports_route_guidance: [flag],
        3 => supports_exit_info: [flag],
        4 => destination_time_zone_offset_minutes: [flag],
    }
}

csm_message! {
    pub struct RouteGuidanceUpdate = 0x5201 {
        0 => display_component_id: [opt u16],
        1 => state: [opt u8],
        2 => maneuver_state: [opt u8],
        3 => current_road_name: [opt str],
        4 => destination_name: [opt str],
        5 => eta: [opt u64],
        6 => time_remaining: [opt u64],
        7 => distance_remaining: [opt u32],
        8 => distance_remaining_display: [opt str],
        9 => distance_remaining_units: [opt u8],
        10 => distance_to_maneuver: [opt u32],
        11 => distance_to_maneuver_display: [opt str],
        12 => distance_to_maneuver_units: [opt u8],
        13 => current_maneuver_list: [opt bytes],
        14 => maneuver_count: [opt u16],
        15 => visible_in_app: [opt bool],
        16 => lane_guidance_index: [opt u16],
        17 => lane_guidance_count: [opt u16],
        18 => lane_guidance_shown: [opt bool],
        19 => source_name: [opt str],
        20 => source_supports_route_guidance: [opt bool],
        21 => destination_time_zone_offset_minutes: [opt i16],
    }
}

csm_message! {
    pub struct RouteGuidanceManeuverUpdate = 0x5202 {
        0 => display_component_id: [opt u16],
        1 => index: [opt u16],
        2 => description: [opt str],
        3 => maneuver_type: [opt u8],
        4 => after_maneuver_road_name: [opt str],
        5 => distance_between: [opt u32],
        6 => distance_between_display: [opt str],
        7 => distance_between_units: [opt u8],
        8 => driving_side: [opt u8],
        9 => junction_type: [opt u8],
        10 => junction_element_angles: [rep i16],
        11 => exit_angle: [opt i16],
        12 => linked_lane_guidance: [opt u16],
        13 => exit_info: [opt str],
    }
}

csm_message! {
    pub struct StopRouteGuidanceUpdates = 0x5203 {
        0 => display_component_id: [opt u16],
    }
}

csm_group! {
    pub struct LaneInformation {
        0 => index: [opt u16],
        1 => status: [opt u8],
        2 => angles: [rep i16],
        3 => highlight_angle: [opt i16],
    }
}

csm_message! {
    pub struct LaneGuidanceInformation = 0x5204 {
        0 => display_component_id: [opt u16],
        1 => index: [opt u16],
        2 => lanes: [rep group LaneInformation],
        3 => description: [opt str],
    }
}
