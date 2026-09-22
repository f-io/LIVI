use crate::csm_message;

csm_message! {
    pub struct StartLocationInformation = 0xFFFA {
        1 => gps_fix_data: [flag],
        2 => recommended_minimum: [flag],
        3 => satellites_in_view: [flag],
        4 => vehicle_speed: [flag],
    }
}

csm_message! {
    pub struct LocationInformation = 0xFFFB {
        0 => nmea_sentence: [str],
    }
}

csm_message! {
    pub struct StopLocationInformation = 0xFFFC {}
}
