use crate::{csm_enum, csm_message};

csm_message! {
    pub struct RequestWiFiInformation = 0x5700 {}
}

csm_enum! {
    pub enum WiFiRequestStatus {
        Success = 0,
        UserDeclined = 1,
        NetworkInformationUnavailable = 2,
    }
}

csm_message! {
    pub struct WiFiInformation = 0x5701 {
        0 => status: [enum WiFiRequestStatus],
        1 => ssid: [opt str],
        2 => passphrase: [opt str],
    }
}

csm_message! {
    pub struct RequestAccessoryWiFiConfigurationInformation = 0x5702 {}
}

csm_enum! {
    pub enum SecurityType {
        None = 0,
        Wep = 1,
        WpaWpa2 = 2,
        Wpa3Transition = 3,
        Wpa3Only = 4,
    }
}

csm_message! {
    pub struct AccessoryWiFiConfigurationInformation = 0x5703 {
        1 => ssid: [opt str],
        2 => passphrase: [opt str],
        3 => security_type: [enum SecurityType],
        4 => channel: [u8],
    }
}
