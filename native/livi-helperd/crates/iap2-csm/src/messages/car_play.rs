use crate::{csm_enum, csm_group, csm_message};

csm_message! {
    pub struct DeviceTransportIdentifierNotification = 0x4E0E {
        0 => bluetooth_transport_id: [str],
        1 => usb_transport_id: [str],
    }
}

csm_enum! {
    pub enum WirelessCarPlayStatus {
        Unavailable = 0,
        Available = 1,
    }
}

csm_message! {
    pub struct WirelessCarPlayUpdate = 0x4E0D {
        0 => status: [enum WirelessCarPlayStatus],
    }
}

csm_group! {
    pub struct CarPlayAvailabilityWiredAttributes {
        0 => available: [opt bool],
        1 => usb_transport_identifier: [opt str],
    }
}

csm_group! {
    pub struct CarPlayAvailabilityWirelessAttributes {
        0 => available: [opt bool],
        1 => bluetooth_transport_identifier: [opt str],
    }
}

csm_message! {
    pub struct CarPlayAvailability = 0x4300 {
        0 => wired_attributes: [opt group CarPlayAvailabilityWiredAttributes],
        1 => wireless_attributes: [opt group CarPlayAvailabilityWirelessAttributes],
    }
}

csm_group! {
    pub struct CarPlayStartSessionWiredAttributes {
        0 => ip_address: [list str],
    }
}

csm_group! {
    pub struct CarPlayStartSessionWirelessAttributes {
        0 => wifi_ssid: [opt str],
        1 => passphrase: [opt str],
        2 => channel: [opt u8],
        3 => ip_address: [list str],
        4 => security_type: [opt u8],
    }
}

csm_message! {
    pub struct CarPlayStartSession = 0x4301 {
        0 => wired_attributes: [opt group CarPlayStartSessionWiredAttributes],
        1 => wireless_attributes: [opt group CarPlayStartSessionWirelessAttributes],
        2 => port: [opt u32],
        3 => device_identifier: [opt str],
        4 => public_key: [opt str],
        5 => source_version: [opt str],
    }
}
