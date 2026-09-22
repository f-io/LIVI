use iap2_csm::messages::car_play::*;
use iap2_csm::messages::vehicle_status::*;
use iap2_csm::{frame_header, CsmMessage, Error};

#[test]
fn missing_required_param_errors() {
    let frame = [0x40, 0x40, 0x00, 0x06, 0x4E, 0x0D];
    let err = WirelessCarPlayUpdate::decode(&frame).unwrap_err();
    assert_eq!(err, Error::MissingParam { message: "WirelessCarPlayUpdate", param: "status" });
}

#[test]
fn unknown_params_are_ignored() {
    let mut frame = vec![0x40, 0x40, 0x00, 0x11, 0xA1, 0x01];
    frame.extend_from_slice(&[0x00, 0x05, 0x00, 0x63, 0xEE]);
    frame.extend_from_slice(&[0x00, 0x06, 0x00, 0x03, 0x00, 0xFA]);
    let m = VehicleStatusUpdate::decode(&frame).unwrap();
    assert_eq!(m.range, Some(250));
    assert_eq!(m.outside_temperature, None);
}

#[test]
fn duplicate_param_first_wins() {
    let mut frame = vec![0x40, 0x40, 0x00, 0x12, 0xA1, 0x01];
    frame.extend_from_slice(&[0x00, 0x06, 0x00, 0x03, 0x00, 0x01]);
    frame.extend_from_slice(&[0x00, 0x06, 0x00, 0x03, 0x00, 0x02]);
    let m = VehicleStatusUpdate::decode(&frame).unwrap();
    assert_eq!(m.range, Some(1));
}

#[test]
fn empty_scalar_payload_decodes_as_none() {
    let mut frame = vec![0x40, 0x40, 0x00, 0x0A, 0xA1, 0x01];
    frame.extend_from_slice(&[0x00, 0x04, 0x00, 0x03]);
    let m = VehicleStatusUpdate::decode(&frame).unwrap();
    assert_eq!(m.range, None);
}

#[test]
fn wrong_msg_id_errors() {
    let frame = [0x40, 0x40, 0x00, 0x06, 0xA1, 0x00];
    let err = WirelessCarPlayUpdate::decode(&frame).unwrap_err();
    assert_eq!(err, Error::WrongMsgId { expected: 0x4E0D, got: 0xA100 });
}

#[test]
fn bad_start_marker_errors() {
    assert_eq!(frame_header(&[0x40, 0x41, 0x00, 0x06, 0xA1, 0x00]), Err(Error::Header));
}

#[test]
fn empty_list_is_omitted() {
    let m = CarPlayStartSession {
        wired_attributes: Some(CarPlayStartSessionWiredAttributes { ip_address: vec![] }),
        wireless_attributes: None,
        port: None,
        device_identifier: None,
        public_key: None,
        source_version: None,
    };
    assert_eq!(m.encode(), vec![0x40, 0x40, 0x00, 0x0A, 0x43, 0x01, 0x00, 0x04, 0x00, 0x00]);
}
