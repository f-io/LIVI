// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::csm_message;
use crate::messages::identification::ChargingConnectorType;

csm_message! {
    pub struct StartVehicleStatusUpdates = 0xA100 {}
}

csm_message! {
    pub struct VehicleStatusUpdate = 0xA101 {
        3 => range: [opt u16],
        4 => outside_temperature: [opt i16],
        6 => range_warning: [opt bool],
        9 => range_gasoline: [opt u16],
        10 => range_diesel: [opt u16],
        11 => range_electric: [opt u16],
        12 => range_cng: [opt u16],
        13 => range_warning_gasoline: [opt bool],
        14 => range_warning_diesel: [opt bool],
        15 => range_warning_electric: [opt bool],
        16 => range_warning_cng: [opt bool],
        21 => min_battery_charge_wh: [opt u32],
        22 => current_battery_charge_wh: [opt u32],
        23 => max_battery_charge_wh: [opt u32],
        24 => displayed_battery_percentage_e3: [opt u32],
        25 => is_charging: [opt bool],
        28 => active_connector: [opt enum ChargingConnectorType],
        30 => max_range_electric: [opt u16],
    }
}

csm_message! {
    pub struct StopVehicleStatusUpdates = 0xA102 {}
}
