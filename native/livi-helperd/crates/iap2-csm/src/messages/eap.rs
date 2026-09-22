use crate::{csm_enum, csm_message};

csm_message! {
    pub struct StartExternalAccessoryProtocolSession = 0xEA00 {
        0 => protocol_id: [u8],
        1 => session_id: [u16],
    }
}

csm_message! {
    pub struct StopExternalAccessoryProtocolSession = 0xEA01 {
        0 => session_id: [u16],
    }
}

csm_enum! {
    pub enum SessionStatus {
        Ok = 0,
        Close = 1,
    }
}

csm_message! {
    pub struct StatusExternalAccessoryProtocolSession = 0xEA03 {
        0 => session_id: [u16],
        1 => status: [enum SessionStatus],
    }
}
