use crate::csm_message;

csm_message! {
    pub struct StartRouteGuidanceUpdates = 0x5200 {
        0 => display_component_id: [opt u16],
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
        10 => distance_to_maneuver: [opt u32],
        13 => current_maneuver_list: [opt bytes],
    }
}

csm_message! {
    pub struct RouteGuidanceManeuverUpdate = 0x5202 {
        0 => display_component_id: [opt u16],
        1 => index: [opt u16],
        3 => maneuver_type: [opt u8],
        4 => after_maneuver_road_name: [opt str],
        8 => driving_side: [opt u8],
        9 => junction_type: [opt u8],
        11 => exit_angle: [opt i16],
    }
}

csm_message! {
    pub struct StopRouteGuidanceUpdates = 0x5203 {
        0 => display_component_id: [opt u16],
    }
}
