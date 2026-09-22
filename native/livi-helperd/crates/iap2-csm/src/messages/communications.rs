use crate::csm_message;

csm_message! {
    pub struct StartCallStateUpdates = 0x4154 {
        0 => remote_id: [flag],
        1 => display_name: [flag],
        2 => status: [flag],
        3 => direction: [flag],
        4 => call_uuid: [flag],
        6 => address_book_id: [flag],
        7 => label: [flag],
        8 => service: [flag],
        9 => is_conferenced: [flag],
        10 => conference_group: [flag],
        11 => disconnect_reason: [flag],
        12 => start_timestamp: [flag],
    }
}

csm_message! {
    pub struct CallStateUpdate = 0x4155 {
        0 => remote_id: [opt str],
        1 => display_name: [opt str],
        2 => status: [opt u8],
        3 => direction: [opt u8],
        4 => call_uuid: [opt str],
        11 => disconnect_reason: [opt u8],
    }
}

csm_message! {
    pub struct StopCallStateUpdates = 0x4156 {}
}

csm_message! {
    pub struct StartCommunicationsUpdates = 0x4157 {
        0 => signal_strength: [flag],
        1 => registration_status: [flag],
        2 => airplane_mode_status: [flag],
        4 => carrier_name: [flag],
        5 => cellular_supported: [flag],
    }
}

csm_message! {
    pub struct CommunicationsUpdate = 0x4158 {
        0 => signal_strength: [opt bytes],
        1 => registration_status: [opt bytes],
        2 => airplane_mode_status: [opt bool],
        4 => carrier_name: [opt str],
        5 => cellular_supported: [opt bool],
    }
}

csm_message! {
    pub struct StopCommunicationsUpdates = 0x4159 {}
}
