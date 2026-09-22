use iap2_csm::messages::authentication::*;
use iap2_csm::messages::car_play::*;
use iap2_csm::messages::communications::*;
use iap2_csm::messages::device_notifications::*;
use iap2_csm::messages::eap::*;
use iap2_csm::messages::identification::*;
use iap2_csm::messages::location::*;
use iap2_csm::messages::now_playing::*;
use iap2_csm::messages::power::*;
use iap2_csm::messages::route_guidance::*;
use iap2_csm::messages::vehicle_status::*;
use iap2_csm::messages::wifi::*;
use iap2_csm::CsmMessage;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vectors() -> Vec<(String, Vec<u8>)> {
    include_str!("vectors.txt")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (name, hex) = l.split_once(' ').unwrap();
            (name.to_owned(), unhex(hex))
        })
        .collect()
}

fn roundtrip<M: CsmMessage>(name: &str, frame: &[u8]) -> M {
    let msg = M::decode(frame).unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));
    assert_eq!(msg.encode(), frame, "{name}: re-encode differs from vector");
    msg
}

#[test]
fn vectors_roundtrip() {
    let mut seen = 0;
    for (name, frame) in vectors() {
        match name.split('#').next().unwrap() {
            "RequestAuthenticationCertificate" => {
                roundtrip::<RequestAuthenticationCertificate>(&name, &frame);
            }
            "AuthenticationCertificate" => {
                let m = roundtrip::<AuthenticationCertificate>(&name, &frame);
                assert_eq!(m.certificate, vec![0x01, 0x02, 0xff]);
            }
            "RequestAuthenticationChallengeResponse" => {
                let m = roundtrip::<RequestAuthenticationChallengeResponse>(&name, &frame);
                assert_eq!(m.challenge, vec![0xaa, 0xbb]);
            }
            "AuthenticationResponse" => {
                let m = roundtrip::<AuthenticationResponse>(&name, &frame);
                assert!(m.response.is_empty());
            }
            "AuthenticationSucceeded" => {
                roundtrip::<AuthenticationSucceeded>(&name, &frame);
            }
            "DeviceTransportIdentifierNotification" => {
                let m = roundtrip::<DeviceTransportIdentifierNotification>(&name, &frame);
                assert_eq!(m.bluetooth_transport_id, "AA:BB:CC:DD:EE:FF");
                assert_eq!(m.usb_transport_id, "usb-1");
            }
            "WirelessCarPlayUpdate" => {
                let m = roundtrip::<WirelessCarPlayUpdate>(&name, &frame);
                assert_eq!(m.status, WirelessCarPlayStatus::Available);
            }
            "CarPlayAvailability" => {
                let m = roundtrip::<CarPlayAvailability>(&name, &frame);
                let wired = m.wired_attributes.unwrap();
                assert_eq!(wired.available, Some(true));
                assert_eq!(wired.usb_transport_identifier.as_deref(), Some("usbid"));
                assert_eq!(m.wireless_attributes.unwrap().available, Some(false));
            }
            "CarPlayStartSession" => {
                let m = roundtrip::<CarPlayStartSession>(&name, &frame);
                if name.ends_with("wireless") {
                    let w = m.wireless_attributes.unwrap();
                    assert_eq!(w.wifi_ssid.as_deref(), Some("LIVI"));
                    assert_eq!(w.channel, Some(36));
                    assert_eq!(w.ip_address, vec!["192.168.2.1"]);
                    assert_eq!(m.port, Some(49152));
                }
            }
            "VehicleStatusUpdate" => {
                let m = roundtrip::<VehicleStatusUpdate>(&name, &frame);
                if name.ends_with("full") {
                    assert_eq!(m.range, Some(250));
                    assert_eq!(m.outside_temperature, Some(-5));
                    assert_eq!(m.range_warning, Some(false));
                }
            }
            "StartVehicleStatusUpdates" => {
                roundtrip::<StartVehicleStatusUpdates>(&name, &frame);
            }
            "StartExternalAccessoryProtocolSession" => {
                let m = roundtrip::<StartExternalAccessoryProtocolSession>(&name, &frame);
                assert_eq!((m.protocol_id, m.session_id), (1, 0x1234));
            }
            "StatusExternalAccessoryProtocolSession" => {
                let m = roundtrip::<StatusExternalAccessoryProtocolSession>(&name, &frame);
                assert_eq!(m.status, SessionStatus::Close);
            }
            "WiFiInformation" => {
                let m = roundtrip::<WiFiInformation>(&name, &frame);
                assert_eq!(m.ssid.as_deref(), Some("HomeNet"));
                assert_eq!(m.passphrase, None);
            }
            "AccessoryWiFiConfigurationInformation" => {
                let m = roundtrip::<AccessoryWiFiConfigurationInformation>(&name, &frame);
                assert_eq!(m.security_type, SecurityType::Wpa3Transition);
                assert_eq!(m.channel, 36);
            }
            "StartLocationInformation" => {
                let m = roundtrip::<StartLocationInformation>(&name, &frame);
                assert!(m.gps_fix_data && m.vehicle_speed);
                assert!(!m.recommended_minimum && !m.satellites_in_view);
            }
            "LocationInformation" => {
                let m = roundtrip::<LocationInformation>(&name, &frame);
                assert_eq!(m.nmea_sentence, "$GPGGA,123519,4807.038,N");
            }
            "StartPowerUpdates" => {
                roundtrip::<StartPowerUpdates>(&name, &frame);
            }
            "PowerUpdate" => {
                let m = roundtrip::<PowerUpdate>(&name, &frame);
                assert_eq!(m.maximum_current_drawn_from_accessory, Some(2400));
                assert_eq!(m.is_external_charger_connected, Some(true));
            }
            "PowerSourceUpdate" => {
                roundtrip::<PowerSourceUpdate>(&name, &frame);
            }
            "DeviceTimeUpdate" => {
                let m = roundtrip::<DeviceTimeUpdate>(&name, &frame);
                assert_eq!(m.seconds_since_reference_date, Some(776862000));
                assert_eq!(m.daylight_savings_offset_minutes, Some(-60));
            }
            "RouteGuidanceUpdate" => {
                let m = roundtrip::<RouteGuidanceUpdate>(&name, &frame);
                assert_eq!(m.current_road_name.as_deref(), Some("Hauptstraße"));
                assert_eq!(m.eta, Some(776862000000));
            }
            "RouteGuidanceManeuverUpdate" => {
                let m = roundtrip::<RouteGuidanceManeuverUpdate>(&name, &frame);
                assert_eq!(m.exit_angle, Some(-90));
            }
            "StartCallStateUpdates" => {
                roundtrip::<StartCallStateUpdates>(&name, &frame);
            }
            "CallStateUpdate" => {
                let m = roundtrip::<CallStateUpdate>(&name, &frame);
                assert_eq!(m.remote_id.as_deref(), Some("+491701234567"));
            }
            "CommunicationsUpdate" => {
                let m = roundtrip::<CommunicationsUpdate>(&name, &frame);
                assert_eq!(m.signal_strength, Some(vec![0x04]));
                assert_eq!(m.airplane_mode_status, Some(false));
            }
            "StartNowPlayingUpdates" => {
                let m = roundtrip::<StartNowPlayingUpdates>(&name, &frame);
                assert!(m.media_item_attributes.unwrap().artwork);
            }
            "NowPlayingUpdate" => {
                let m = roundtrip::<NowPlayingUpdate>(&name, &frame);
                let media = m.media_item_attributes.unwrap();
                assert_eq!(media.persistent_id, Some(0xDEADBEEF));
                assert_eq!(media.artwork_ftid, Some(7));
                assert_eq!(m.playback_attributes.unwrap().status, Some(PlaybackStatus::Playing));
            }
            "IdentificationInformation" => {
                let m = roundtrip::<IdentificationInformation>(&name, &frame);
                assert_eq!(m.power_providing_capability, PowerProvidingCapability::Advanced);
                let eap = &m.supported_external_accessory_protocol[0];
                assert_eq!(eap.match_action, MatchAction::NoActionNoCommunication);
                assert!(eap.car_play);
                let usb = &m.usb_host_transport_component[0];
                assert_eq!((usb.id, usb.name.as_str()), (2, "usbhost"));
                assert!(usb.supports_iap2_connection && usb.supports_car_play);
                assert_eq!(usb.car_play_interface_number, Some(0));
                let bt = &m.bluetooth_transport_component[0];
                assert_eq!(bt.bluetooth_transport_mac, vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
                let wcp = m.wireless_car_play_transport_component.unwrap();
                assert!(wcp.supports_car_play && wcp.supports_iap2_connection);
                assert_eq!(m.vehicle_information_component.unwrap().engine_type, EngineType::Electric);
                assert_eq!(m.route_guidance_display_component[0].max_current_road_name_length, Some(50));
            }
            "IdentificationRejected" => {
                let m = roundtrip::<IdentificationRejected>(&name, &frame);
                assert!(m.name && m.serial_number && !m.manufacturer);
            }
            "IdentificationAccepted" => {
                roundtrip::<IdentificationAccepted>(&name, &frame);
            }
            other => panic!("no roundtrip mapping for vector {other}"),
        }
        seen += 1;
    }
    assert_eq!(seen, 33, "vector count changed, update the mapping");
}
