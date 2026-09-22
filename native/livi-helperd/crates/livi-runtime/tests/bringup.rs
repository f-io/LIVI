use tokio::sync::mpsc;

use iap2_csm::messages::authentication::*;
use iap2_csm::messages::identification::*;
use iap2_csm::messages::now_playing::*;
use iap2_csm::messages::power::PowerSourceUpdate;
use iap2_csm::CsmMessage;
use iap2_csm::messages::wifi::SecurityType;
use iap2_csm::messages::location::*;
use iap2_csm::messages::vehicle_status::*;
use livi_runtime::bringup::{run_accessory, BringupEvent, CpConfig};
use livi_runtime::framing::frame_msg_id;
use livi_runtime::ident::{Identity, Transport};
use livi_runtime::vehicle::{Vehicle, VehicleFeed};
use base64::Engine;
use livi_runtime::{AsyncAuth, ChannelError, ControlChannel};

struct PairChannel {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

fn pair() -> (PairChannel, PairChannel) {
    let (a_tx, a_rx) = mpsc::unbounded_channel();
    let (b_tx, b_rx) = mpsc::unbounded_channel();
    (PairChannel { tx: a_tx, rx: b_rx }, PairChannel { tx: b_tx, rx: a_rx })
}

impl ControlChannel for PairChannel {
    async fn send(&mut self, frame: Vec<u8>) -> Result<(), ChannelError> {
        self.tx.send(frame).map_err(|_| ChannelError::Closed)
    }
    async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
    }
}

struct MockAuth {
    cert: Vec<u8>,
}

impl AsyncAuth for MockAuth {
    async fn read_certificate(&mut self) -> Result<Vec<u8>, String> {
        Ok(self.cert.clone())
    }
    async fn sign(&mut self, challenge: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(challenge.iter().rev().copied().collect())
    }
    async fn protocol_major(&mut self) -> Result<u8, String> {
        Ok(3)
    }
}

impl PairChannel {
    async fn expect(&mut self, msg_id: u16) -> Vec<u8> {
        let frame = self.recv().await.expect("phone side channel closed");
        assert_eq!(frame_msg_id(&frame), Some(msg_id), "unexpected accessory message");
        frame
    }
}

fn identity() -> Identity {
    Identity { name: "LIVI".into(), ssid: "LIVI".into(), bt_mac: [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33] }
}

fn cp_config() -> CpConfig {
    CpConfig {
        ap_mac: None,
        wifi_iface: "nonexistent0".into(),
        ssid: "LIVI".into(),
        passphrase: "12345678".into(),
        channel: 36,
        security_type: SecurityType::WpaWpa2,
        airplay_port: 7000,
        source_version: "950.7.1".into(),
        public_key: String::new(),
        transport: Transport::Wireless,
        av_iface: None,
        available_current_ma: 500,
    }
}

#[tokio::test]
async fn full_bringup_sequence() {
    let (accessory, mut phone) = pair();
    let (tx, mut rx) = mpsc::channel(32);
    let auth = MockAuth { cert: vec![0xDE, 0xAD, 0xBE, 0xEF] };
    let handle = tokio::spawn(run_accessory(accessory, auth, identity(), cp_config(), tx, VehicleFeed::quiet()));

    phone.send(StartIdentification {}.encode()).await.unwrap();
    let ident_frame = phone.expect(0x1D01).await;
    let ident = IdentificationInformation::decode(&ident_frame).unwrap();
    assert_eq!(ident.name, "LIVI");
    assert_eq!(ident.bluetooth_transport_component[0].bluetooth_transport_mac, identity().bt_mac);
    assert!(ident.vehicle_information_component.is_some());

    phone.send(IdentificationAccepted {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Identified));

    phone.send(RequestAuthenticationCertificate {}.encode()).await.unwrap();
    let cert_frame = phone.expect(0xAA01).await;
    assert_eq!(AuthenticationCertificate::decode(&cert_frame).unwrap().certificate, vec![0xDE, 0xAD, 0xBE, 0xEF]);

    let challenge = vec![1u8, 2, 3, 4];
    phone
        .send(RequestAuthenticationChallengeResponse { challenge: challenge.clone() }.encode())
        .await
        .unwrap();
    let resp_frame = phone.expect(0xAA03).await;
    let resp = AuthenticationResponse::decode(&resp_frame).unwrap();
    assert_eq!(resp.response, vec![4, 3, 2, 1]);

    phone.send(AuthenticationSucceeded {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Authenticated));

    for expected in [0x5000, 0x5200, 0xAE00, 0x4157, 0x4154] {
        phone.expect(expected).await;
    }
    assert_eq!(rx.recv().await, Some(BringupEvent::Subscribed));

    phone
        .send(
            NowPlayingUpdate {
                media_item_attributes: Some(MediaItemAttributes {
                    persistent_id: None,
                    title: Some("Song".into()),
                    duration_ms: Some(180000),
                    album: None,
                    artist: Some("Artist".into()),
                    album_artist: None,
                    genre: None,
                    artwork_ftid: None,
                }),
                playback_attributes: None,
            }
            .encode(),
        )
        .await
        .unwrap();
    match rx.recv().await {
        Some(BringupEvent::Incoming { msg_id, .. }) => assert_eq!(msg_id, 0x5001),
        other => panic!("expected incoming 0x5001, got {other:?}"),
    }

    drop(phone);
    assert_eq!(rx.recv().await, Some(BringupEvent::Closed));
    handle.await.unwrap();
}

#[tokio::test]
async fn wired_identifies_over_usb_and_offers_power() {
    let (accessory, mut phone) = pair();
    let (tx, mut rx) = mpsc::channel(32);
    let cp = CpConfig { transport: Transport::Wired, ..cp_config() };
    tokio::spawn(run_accessory(accessory, MockAuth { cert: vec![0x01] }, identity(), cp, tx, VehicleFeed::quiet()));

    phone.send(StartIdentification {}.encode()).await.unwrap();
    let ident = IdentificationInformation::decode(&phone.expect(0x1D01).await).unwrap();
    assert!(ident.bluetooth_transport_component.is_empty(), "wired must not offer BT transport");
    assert!(ident.wireless_car_play_transport_component.is_none());
    let usb = &ident.usb_host_transport_component[0];
    assert_eq!(usb.car_play_interface_number, Some(3));
    assert!(usb.supports_car_play);

    phone.send(IdentificationAccepted {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Identified));

    phone.send(AuthenticationSucceeded {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Authenticated));

    let power = PowerSourceUpdate::decode(&phone.expect(0xAE03).await).unwrap();
    assert_eq!(power.available_current_for_device, Some(500));
    assert_eq!(power.device_battery_should_charge_if_power_is_present, Some(true));
}

#[tokio::test]
async fn identification_retries_without_droppable_field() {
    let (accessory, mut phone) = pair();
    let (tx, mut rx) = mpsc::channel(32);
    let auth = MockAuth { cert: vec![0x01] };
    tokio::spawn(run_accessory(accessory, auth, identity(), cp_config(), tx, VehicleFeed::quiet()));

    phone.send(StartIdentification {}.encode()).await.unwrap();
    let first = phone.expect(0x1D01).await;
    assert!(IdentificationInformation::decode(&first).unwrap().vehicle_status_component.is_some());

    let mut reject = IdentificationRejected {
        name: false,
        model_identifier: false,
        manufacturer: false,
        serial_number: false,
        fireware_version: false,
        hardware_version: false,
        messages_sent_by_accessory: false,
        messages_received_from_accessory: false,
        power_providing_capability: false,
        maximum_current_drawn_from_device: false,
        supported_external_accessory_protocol: false,
        app_match_team_id: false,
        current_language: false,
        supported_language: false,
        serial_transport_component: false,
        usb_device_transport_component: false,
        usb_host_transport_component: false,
        bluetooth_transport_component: false,
        vehicle_information_component: false,
        vehicle_status_component: false,
        location_information_component: false,
        wireless_car_play_transport_component: false,
    };
    reject.vehicle_status_component = true;
    phone.send(reject.encode()).await.unwrap();

    let retry = phone.expect(0x1D01).await;
    assert!(IdentificationInformation::decode(&retry).unwrap().vehicle_status_component.is_none());

    phone.send(IdentificationAccepted {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Identified));
}

/// Identification, auth and the subscriptions, so a test can start at the running phase.
async fn bring_up(phone: &mut PairChannel, rx: &mut mpsc::Receiver<BringupEvent>) {
    phone.send(StartIdentification {}.encode()).await.unwrap();
    phone.expect(0x1D01).await;
    phone.send(IdentificationAccepted {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Identified));
    phone.send(RequestAuthenticationCertificate {}.encode()).await.unwrap();
    phone.expect(0xAA01).await;
    phone.send(RequestAuthenticationChallengeResponse { challenge: vec![1] }.encode()).await.unwrap();
    phone.expect(0xAA03).await;
    phone.send(AuthenticationSucceeded {}.encode()).await.unwrap();
    assert_eq!(rx.recv().await, Some(BringupEvent::Authenticated));
    for expected in [0x5000, 0x5200, 0xAE00, 0x4157, 0x4154] {
        phone.expect(expected).await;
    }
    assert_eq!(rx.recv().await, Some(BringupEvent::Subscribed));
}

#[tokio::test]
async fn location_and_vehicle_status_reach_the_phone_that_asked() {
    let (accessory, mut phone) = pair();
    let (tx, mut rx) = mpsc::channel(32);
    let vehicle = Vehicle::default();
    tokio::spawn(run_accessory(accessory, MockAuth { cert: vec![1] }, identity(), cp_config(), tx, vehicle.feed()));
    bring_up(&mut phone, &mut rx).await;

    // Pushed before anyone asked: kept, not sent.
    vehicle.push_status(r#"{"range":320,"outsideTemperature":12}"#).unwrap();
    let block = base64::engine::general_purpose::STANDARD.encode("$GPGGA,1*00\r\n$GPRMC,2*00\r\n$GPGSV,3*00\r\n");
    vehicle.push_location(&block).unwrap();

    phone.send(StartVehicleStatusUpdates {}.encode()).await.unwrap();
    let status = VehicleStatusUpdate::decode(&phone.expect(0xA101).await).unwrap();
    assert_eq!((status.range, status.outside_temperature, status.range_warning), (Some(320), Some(12), None));

    phone
        .send(
            StartLocationInformation {
                gps_fix_data: true,
                recommended_minimum: true,
                satellites_in_view: false,
                vehicle_speed: false,
            }
            .encode(),
        )
        .await
        .unwrap();
    while !matches!(rx.recv().await, Some(BringupEvent::Incoming { msg_id: 0xFFFA, .. })) {}

    vehicle.push_location(&block).unwrap();
    let first = LocationInformation::decode(&phone.expect(0xFFFB).await).unwrap();
    let second = LocationInformation::decode(&phone.expect(0xFFFB).await).unwrap();
    assert_eq!((first.nmea_sentence.as_str(), second.nmea_sentence.as_str()), ("$GPGGA,1*00", "$GPRMC,2*00"));

    // A changed value goes out by itself, the same one again does not.
    vehicle.push_status(r#"{"range":300}"#).unwrap();
    let status = VehicleStatusUpdate::decode(&phone.expect(0xA101).await).unwrap();
    assert_eq!((status.range, status.outside_temperature), (Some(300), Some(12)));

    phone.send(StopLocationInformation {}.encode()).await.unwrap();
    phone.send(StopVehicleStatusUpdates {}.encode()).await.unwrap();
    while !matches!(rx.recv().await, Some(BringupEvent::Incoming { msg_id: 0xA102, .. })) {}
    vehicle.push_location(&block).unwrap();
    vehicle.push_status(r#"{"range":290}"#).unwrap();
    // The GSV the phone never asked for is filtered even while subscribed; after the stop nothing
    // at all arrives, which the next expected message proves.
    phone.send(StartVehicleStatusUpdates {}.encode()).await.unwrap();
    let status = VehicleStatusUpdate::decode(&phone.expect(0xA101).await).unwrap();
    assert_eq!(status.range, Some(290));
}
