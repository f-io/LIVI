use crate::csm_message;

csm_message! {
    pub struct StartVehicleStatusUpdates = 0xA100 {}
}

csm_message! {
    pub struct VehicleStatusUpdate = 0xA101 {
        3 => range: [opt u16],
        4 => outside_temperature: [opt i16],
        6 => range_warning: [opt bool],
    }
}

csm_message! {
    pub struct StopVehicleStatusUpdates = 0xA102 {}
}
