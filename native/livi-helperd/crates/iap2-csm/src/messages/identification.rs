use crate::{csm_enum, csm_field_decode, csm_field_encode, csm_group, csm_message, CsmParams, Error};

csm_enum! {
    pub enum PowerProvidingCapability {
        None = 0,
        Reserved = 1,
        Advanced = 2,
    }
}

csm_enum! {
    pub enum MatchAction {
        None = 0,
        SettingsAndPrompt = 1,
        SettingsOnly = 2,
        NoActionNoCommunication = 4,
    }
}

csm_enum! {
    pub enum EngineType {
        Gas = 0,
        Diesel = 1,
        Electric = 2,
        Cng = 3,
    }
}

csm_group! {
    pub struct ExternalAccessoryProtocol {
        0 => id: [u8],
        1 => name: [str],
        2 => match_action: [enum MatchAction],
        3 => native_transport_component_identifier: [opt u16],
        4 => car_play: [flag],
    }
}

// Transport components emit their base params (id, name, iap2 flag) twice: as ids 0-2 and
// again in the 3-5 range, with subclass params overriding single ids in that range.
macro_rules! transport_component {
    (
        $name:ident {
            $( $pid:literal => $f:ident: [$($k:tt)+] ),* $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            pub id: u16,
            pub name: String,
            pub supports_iap2_connection: bool,
            $( pub $f: $crate::csm_field_ty!($($k)+), )*
        }

        impl CsmParams for $name {
            fn encode_params(&self, out: &mut Vec<u8>) {
                csm_field_encode!(out, 0, self.id, u16);
                csm_field_encode!(out, 1, self.name, str);
                csm_field_encode!(out, 2, self.supports_iap2_connection, flag);
                transport_component!(@second out, self, { $( $pid => $f: [$($k)+] ),* });
            }

            fn decode_params(params: &[(u16, &[u8])]) -> Result<Self, Error> {
                Ok(Self {
                    id: csm_field_decode!(params, 0, stringify!($name), "id", u16)?,
                    name: csm_field_decode!(params, 1, stringify!($name), "name", str)?,
                    supports_iap2_connection:
                        csm_field_decode!(params, 2, stringify!($name), "supports_iap2_connection", flag)?,
                    $( $f: csm_field_decode!(
                        params, $pid, stringify!($name), stringify!($f), $($k)+)?, )*
                })
            }
        }
    };
    (@second $out:ident, $self:ident, { $( $pid:literal => $f:ident: [$($k:tt)+] ),* }) => {
        for slot in 3u16..=5 {
            #[allow(unreachable_patterns)]
            match slot {
                $( $pid => csm_field_encode!($out, $pid, $self.$f, $($k)+), )*
                3 => csm_field_encode!($out, 3, $self.id, u16),
                4 => csm_field_encode!($out, 4, $self.name, str),
                _ => csm_field_encode!($out, 5, $self.supports_iap2_connection, flag),
            }
        }
    };
}

transport_component! {
    SerialTransportComponent {}
}

transport_component! {
    BluetoothTransportComponent {
        3 => bluetooth_transport_mac: [bytes],
    }
}

transport_component! {
    USBDeviceTransportComponent {
        3 => audio_sample_rate: [opt u8],
    }
}

transport_component! {
    WirelessCarPlayTransportComponent {
        4 => supports_car_play: [flag],
    }
}

transport_component! {
    USBHostTransportComponent {
        3 => car_play_interface_number: [opt u8],
        4 => supports_car_play: [flag],
    }
}

csm_group! {
    pub struct VehicleInformationComponent {
        0 => id: [u16],
        1 => name: [str],
        2 => engine_type: [enum EngineType],
        6 => display_name: [opt str],
        8 => maps_display_name: [opt str],
    }
}

csm_group! {
    pub struct VehicleStatusComponent {
        0 => id: [u16],
        1 => name: [str],
        3 => range: [flag],
        4 => outside_temperature: [flag],
        6 => range_warning: [flag],
    }
}

csm_group! {
    pub struct RouteGuidanceDisplayComponent {
        0 => id: [u16],
        1 => name: [str],
        2 => max_current_road_name_length: [opt u16],
        3 => max_destination_name_length: [opt u16],
        4 => max_after_maneuver_road_name_length: [opt u16],
        5 => max_maneuver_description_length: [opt u16],
        6 => max_guidance_maneuver_storage_capacity: [opt u16],
        7 => max_lane_guidance_description_length: [opt u16],
        8 => max_lane_guidance_storage_capacity: [opt u16],
    }
}

csm_group! {
    pub struct LocationInformationComponent {
        0 => id: [u16],
        1 => name: [str],
        17 => global_positioning_system_fix_data: [flag],
        18 => recommended_minimum_specific_gps_transit_data: [flag],
    }
}

csm_message! {
    pub struct StartIdentification = 0x1D00 {}
}

csm_message! {
    pub struct IdentificationInformation = 0x1D01 {
        0 => name: [str],
        1 => model_identifier: [str],
        2 => manufacturer: [str],
        3 => serial_number: [str],
        4 => firmware_version: [str],
        5 => hardware_version: [str],
        6 => messages_sent_by_accessory: [bytes],
        7 => messages_received_from_accessory: [bytes],
        8 => power_providing_capability: [enum PowerProvidingCapability],
        9 => maximum_current_drawn_from_device: [u16],
        10 => supported_external_accessory_protocol: [list group ExternalAccessoryProtocol],
        11 => app_match_team_id: [opt str],
        12 => current_language: [str],
        13 => supported_language: [list str],
        14 => serial_transport_component: [list group SerialTransportComponent],
        15 => usb_device_transport_component: [list group USBDeviceTransportComponent],
        16 => usb_host_transport_component: [list group USBHostTransportComponent],
        17 => bluetooth_transport_component: [list group BluetoothTransportComponent],
        20 => vehicle_information_component: [opt group VehicleInformationComponent],
        21 => vehicle_status_component: [opt group VehicleStatusComponent],
        22 => location_information_component: [opt group LocationInformationComponent],
        24 => wireless_car_play_transport_component: [opt group WirelessCarPlayTransportComponent],
        30 => route_guidance_display_component: [list group RouteGuidanceDisplayComponent],
    }
}

csm_message! {
    pub struct IdentificationAccepted = 0x1D02 {}
}

csm_message! {
    pub struct IdentificationRejected = 0x1D03 {
        0 => name: [flag],
        1 => model_identifier: [flag],
        2 => manufacturer: [flag],
        3 => serial_number: [flag],
        4 => fireware_version: [flag],
        5 => hardware_version: [flag],
        6 => messages_sent_by_accessory: [flag],
        7 => messages_received_from_accessory: [flag],
        8 => power_providing_capability: [flag],
        9 => maximum_current_drawn_from_device: [flag],
        10 => supported_external_accessory_protocol: [flag],
        11 => app_match_team_id: [flag],
        12 => current_language: [flag],
        13 => supported_language: [flag],
        14 => serial_transport_component: [flag],
        15 => usb_device_transport_component: [flag],
        16 => usb_host_transport_component: [flag],
        17 => bluetooth_transport_component: [flag],
        20 => vehicle_information_component: [flag],
        21 => vehicle_status_component: [flag],
        22 => location_information_component: [flag],
        24 => wireless_car_play_transport_component: [flag],
    }
}
