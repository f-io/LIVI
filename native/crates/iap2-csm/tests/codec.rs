// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use iap2_csm::messages::car_play::*;
use iap2_csm::messages::route_guidance::*;
use iap2_csm::messages::vehicle_status::*;
use iap2_csm::{CsmMessage, Error, frame_header};

#[test]
fn missing_required_param_errors() {
    let frame = [0x40, 0x40, 0x00, 0x06, 0x4E, 0x0D];
    let err = WirelessCarPlayUpdate::decode(&frame).unwrap_err();
    assert_eq!(
        err,
        Error::MissingParam {
            message: "WirelessCarPlayUpdate",
            param: "status"
        }
    );
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
    assert_eq!(
        err,
        Error::WrongMsgId {
            expected: 0x4E0D,
            got: 0xA100
        }
    );
}

#[test]
fn bad_start_marker_errors() {
    assert_eq!(
        frame_header(&[0x40, 0x41, 0x00, 0x06, 0xA1, 0x00]),
        Err(Error::Header)
    );
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
        sdk_version: None,
        asset: None,
        supports_mutual_authentication: None,
    };
    assert_eq!(
        m.encode(),
        vec![0x40, 0x40, 0x00, 0x0A, 0x43, 0x01, 0x00, 0x04, 0x00, 0x00]
    );
}

#[test]
fn repeated_params_keep_every_value() {
    let lanes = LaneGuidanceInformation {
        display_component_id: Some(0),
        index: Some(3),
        lanes: vec![
            LaneInformation {
                index: Some(0),
                status: Some(0),
                angles: vec![0],
                highlight_angle: None,
            },
            LaneInformation {
                index: Some(1),
                status: Some(2),
                angles: vec![0, 45],
                highlight_angle: Some(45),
            },
        ],
        description: Some("Use the right lane".into()),
    };
    let frame = lanes.encode();
    let (_, payload) = frame_header(&frame).unwrap();
    let params = iap2_csm::split_params(payload);
    assert_eq!(params.iter().filter(|(id, _)| *id == 2).count(), 2);
    assert_eq!(LaneGuidanceInformation::decode(&frame).unwrap(), lanes);

    let mut frame = vec![0x40, 0x40, 0x00, 0x00, 0x52, 0x02];
    for (id, value) in [(1u16, 7u16), (10, 0xffa6), (10, 0x002d)] {
        frame.extend_from_slice(&6u16.to_be_bytes());
        frame.extend_from_slice(&id.to_be_bytes());
        frame.extend_from_slice(&value.to_be_bytes());
    }
    let len = frame.len() as u16;
    frame[2..4].copy_from_slice(&len.to_be_bytes());
    let turn = RouteGuidanceManeuverUpdate::decode(&frame).unwrap();
    assert_eq!(turn.index, Some(7));
    assert_eq!(turn.junction_element_angles, vec![-90, 45]);
}
