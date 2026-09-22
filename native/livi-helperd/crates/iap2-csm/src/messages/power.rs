use crate::csm_message;

csm_message! {
    pub struct StartPowerUpdates = 0xAE00 {
        0 => maximum_current_drawn_from_accessory: [flag],
        1 => device_battery_will_charge_if_power_is_present: [flag],
        2 => accessory_power_mode: [flag],
        4 => is_external_charger_connected: [flag],
        5 => battery_charging_state: [flag],
        6 => battery_charge_level: [flag],
    }
}

csm_message! {
    pub struct PowerUpdate = 0xAE01 {
        0 => maximum_current_drawn_from_accessory: [opt u16],
        1 => device_battery_will_charge_if_power_is_present: [opt bool],
        2 => accessory_power_mode: [opt u8],
        4 => is_external_charger_connected: [opt bool],
        5 => battery_charging_state: [opt u8],
        6 => battery_charge_level: [opt u16],
    }
}

csm_message! {
    pub struct StopPowerUpdates = 0xAE02 {}
}

csm_message! {
    pub struct PowerSourceUpdate = 0xAE03 {
        0 => available_current_for_device: [opt u16],
        1 => device_battery_should_charge_if_power_is_present: [opt bool],
    }
}
