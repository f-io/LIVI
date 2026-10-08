use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Notify, mpsc, watch};
use tokio::time::Instant;

use iap2_csm::CsmMessage;
use iap2_csm::messages::authentication::*;
use iap2_csm::messages::car_play::*;
use iap2_csm::messages::communications::*;
use iap2_csm::messages::identification::*;
use iap2_csm::messages::location::*;
use iap2_csm::messages::now_playing::*;
use iap2_csm::messages::power::*;
use iap2_csm::messages::route_guidance::*;
use iap2_csm::messages::wifi::*;
use livi_wifi::{Channel, OnAir, Security};

use crate::framing::frame_msg_id;
use crate::ident::{DROPPABLE, Identity, Transport, build_identification};
use crate::vehicle::{Fuels, LocationTypes, VehicleFeed, VehicleStatus};
use crate::{AsyncAuth, ControlChannel, net};

/// Wireless CarPlay parameters handed to the phone: the AP and the AirPlay receiver.
#[derive(Debug, Clone)]
pub struct CpConfig {
    pub wifi_iface: String,
    pub ssid: String,
    pub passphrase: String,
    /// What the phone is told when the access point does not say what it is on air with.
    pub channel: Channel,
    pub airplay_port: u32,
    pub source_version: String,
    pub public_key: String,
    pub transport: Transport,
    /// Wired only: the USB network interface carrying the AV stream, whose link-local
    /// address the phone connects back to.
    pub av_iface: Option<String>,
    /// Wired only: where `av_iface` arrives when it is found after the session started. An iPhone
    /// on a Mac brings its USB network function up only once iAP2 runs over the cable.
    pub av_iface_late: Option<watch::Receiver<Option<String>>>,
    pub available_current_ma: u16,
    /// The access point's MAC when it is not this host's, so the phone is told the right one.
    pub ap_mac: Option<String>,
    /// Asks an access point that is not this host's what it is on air with.
    pub ap_on_air: Option<AskOnAir>,
    /// Bluetooth only: tells whether the phone, by its Bluetooth MAC, runs iAP2 over the cable.
    pub on_cable: Option<OnCable>,
    /// Wired only: fires when the phone should hear the start once more, over the iAP2 session it
    /// already has.
    pub start_again: Option<Arc<Notify>>,
}

pub type AskOnAir = fn() -> Option<OnAir>;

#[derive(Clone)]
pub struct OnCable(pub Arc<dyn Fn(&str) -> bool + Send + Sync>);

impl core::fmt::Debug for OnCable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("OnCable")
    }
}

/// The id every session start hands the phone for this accessory.
pub fn accessory_id(cp: &CpConfig) -> Option<String> {
    cp.ap_mac.clone().or_else(|| net::wlan_mac(&cp.wifi_iface))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BringupEvent {
    Identified,
    Authenticated,
    Subscribed,
    WifiConfigSent,
    CarPlayStartSent { ip: String },
    Incoming { msg_id: u16, frame: Vec<u8> },
    Failed(String),
    Closed,
}

#[derive(Debug)]
pub enum BringupError {
    Channel,
    Identification(String),
    Auth(String),
}

impl core::fmt::Display for BringupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BringupError::Channel => write!(f, "control channel closed during bring-up"),
            BringupError::Identification(e) => write!(f, "identification failed: {e}"),
            BringupError::Auth(e) => write!(f, "authentication failed: {e}"),
        }
    }
}

impl std::error::Error for BringupError {}

fn subscriptions() -> Vec<Vec<u8>> {
    vec![
        StartNowPlayingUpdates {
            media_item_attributes: Some(StartMediaItemAttributes {
                persistent_id: false,
                title: true,
                duration_ms: true,
                album: true,
                artist: true,
                album_artist: false,
                genre: false,
                artwork: true,
            }),
            playback_attributes: Some(StartPlaybackAttributes {
                status: true,
                elapsed_ms: true,
                app_name: true,
                app_bundle_id: false,
            }),
        }
        .encode(),
        StartRouteGuidanceUpdates { display_component_id: None }.encode(),
        StartPowerUpdates {
            maximum_current_drawn_from_accessory: false,
            device_battery_will_charge_if_power_is_present: false,
            accessory_power_mode: false,
            is_external_charger_connected: true,
            battery_charging_state: true,
            battery_charge_level: true,
        }
        .encode(),
        StartCommunicationsUpdates {
            signal_strength: true,
            registration_status: false,
            airplane_mode_status: false,
            carrier_name: true,
            cellular_supported: true,
        }
        .encode(),
        StartCallStateUpdates {
            remote_id: true,
            display_name: true,
            status: true,
            direction: true,
            call_uuid: true,
            address_book_id: false,
            label: false,
            service: false,
            is_conferenced: false,
            conference_group: false,
            disconnect_reason: true,
            start_timestamp: false,
        }
        .encode(),
    ]
}

async fn recv<C: ControlChannel>(ch: &mut C) -> Result<Vec<u8>, BringupError> {
    ch.recv().await.ok_or(BringupError::Channel)
}

async fn run_identification<C: ControlChannel>(
    ch: &mut C,
    id: &Identity,
    transport: Transport,
) -> Result<(), BringupError> {
    let mut exclude: Vec<&str> = Vec::new();
    loop {
        let frame = recv(ch).await?;
        match frame_msg_id(&frame) {
            Some(0x1D00) => {
                let ident = build_identification(id, transport, &exclude);
                ch.send(ident.encode()).await.map_err(|_| BringupError::Channel)?;
            }
            Some(0x1D02) => return Ok(()),
            Some(0x1D03) => {
                let rejected = IdentificationRejected::decode(&frame)
                    .map_err(|e| BringupError::Identification(e.to_string()))?;
                let flagged = flagged_fields(&rejected);
                let drop: Vec<&str> = DROPPABLE
                    .iter()
                    .copied()
                    .filter(|f| flagged.contains(f) && !exclude.contains(f))
                    .collect();
                if drop.is_empty() {
                    return Err(BringupError::Identification(format!(
                        "rejected fields not droppable: {flagged:?}"
                    )));
                }
                exclude.extend(drop);
                let ident = build_identification(id, transport, &exclude);
                ch.send(ident.encode()).await.map_err(|_| BringupError::Channel)?;
            }
            other => {
                return Err(BringupError::Identification(format!(
                    "unexpected message during identification: {other:?}"
                )));
            }
        }
    }
}

fn flagged_fields(r: &IdentificationRejected) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut push = |on: bool, name| {
        if on {
            out.push(name)
        }
    };
    push(r.location_information_component, "location_information_component");
    push(r.vehicle_information_component, "vehicle_information_component");
    push(r.vehicle_status_component, "vehicle_status_component");
    out
}

async fn run_auth<C: ControlChannel, A: AsyncAuth>(
    ch: &mut C,
    auth: &mut A,
) -> Result<(), BringupError> {
    let cert = auth.read_certificate().await.map_err(BringupError::Auth)?;
    loop {
        let frame = recv(ch).await?;
        match frame_msg_id(&frame) {
            Some(0xAA00) => {
                ch.send(AuthenticationCertificate { certificate: cert.clone() }.encode())
                    .await
                    .map_err(|_| BringupError::Channel)?;
            }
            Some(0xAA02) => {
                let req = RequestAuthenticationChallengeResponse::decode(&frame)
                    .map_err(|e| BringupError::Auth(e.to_string()))?;
                let sig = auth.sign(req.challenge).await.map_err(BringupError::Auth)?;
                ch.send(AuthenticationResponse { response: sig }.encode())
                    .await
                    .map_err(|_| BringupError::Channel)?;
            }
            Some(0xAA05) => return Ok(()),
            Some(0xAA04) => {
                return Err(BringupError::Auth("device sent AuthenticationFailed".into()));
            }
            other => {
                return Err(BringupError::Auth(format!(
                    "unexpected message during auth: {other:?}"
                )));
            }
        }
    }
}

fn security_type(security: Security) -> SecurityType {
    match security {
        Security::Wpa2 => SecurityType::WpaWpa2,
        Security::Wpa2Wpa3 => SecurityType::Wpa3Transition,
        Security::Wpa3 => SecurityType::Wpa3Only,
    }
}

/// The network the phone is told about: the one on air, else the configured one.
fn told(cp: &CpConfig, live: Option<&OnAir>) -> (String, u8, SecurityType) {
    let ssid = live.map(|ap| ap.ssid.clone()).filter(|s| !s.is_empty());
    let channel = live.map_or(cp.channel, |ap| ap.channel);
    (
        ssid.unwrap_or_else(|| cp.ssid.clone()),
        u8::try_from(channel.number).unwrap_or_default(),
        security_type(live.map(|ap| ap.security).unwrap_or_default()),
    )
}

fn wifi_config(cp: &CpConfig, live: Option<&OnAir>) -> AccessoryWiFiConfigurationInformation {
    let (ssid, channel, security_type) = told(cp, live);
    AccessoryWiFiConfigurationInformation {
        ssid: Some(ssid),
        passphrase: Some(cp.passphrase.clone()),
        security_type,
        channel,
    }
}

/// The phone of a Bluetooth session that runs iAP2 over the cable by now. A start over Bluetooth
/// would pull it off the cable, so the session ends instead.
fn on_cable(cp: &CpConfig, frame: &[u8]) -> Option<String> {
    let ask = cp.on_cable.as_ref()?;
    let offer = CarPlayAvailability::decode(frame).ok()?;
    let phone = offer.wireless_attributes?.bluetooth_transport_identifier?;
    (ask.0)(&phone).then_some(phone)
}

fn carplay_start_session(cp: &CpConfig, live: Option<OnAir>) -> Option<CarPlayStartSession> {
    if cp.transport == Transport::Wired {
        // The link-local the phone connects to: the A/V interface's.
        let fe80 = net::wlan_link_local(cp.av_iface.as_deref()?)?;
        return Some(CarPlayStartSession {
            wired_attributes: Some(CarPlayStartSessionWiredAttributes { ip_address: vec![fe80] }),
            wireless_attributes: None,
            port: Some(cp.airplay_port),
            // The accessory the phone knows over Wi-Fi, so the cable is the same car. Without an
            // access point the A/V interface stands in.
            device_identifier: accessory_id(cp)
                .or_else(|| cp.av_iface.as_deref().and_then(net::wlan_mac)),
            public_key: Some(cp.public_key.clone()),
            source_version: Some(cp.source_version.clone()),
        });
    }
    let fe80 = net::wlan_link_local(&cp.wifi_iface)?;
    let (ssid, channel, security_type) = told(cp, live.as_ref());
    Some(CarPlayStartSession {
        wired_attributes: None,
        wireless_attributes: Some(CarPlayStartSessionWirelessAttributes {
            wifi_ssid: Some(ssid),
            passphrase: Some(cp.passphrase.clone()),
            channel: Some(channel),
            ip_address: vec![fe80],
            security_type: Some(security_type as u8),
        }),
        port: Some(cp.airplay_port),
        device_identifier: accessory_id(cp),
        public_key: Some(cp.public_key.clone()),
        source_version: Some(cp.source_version.clone()),
    })
}

/// How long the access point is given to come back before the phone is answered anyway.
// Covers the AP service's own readiness budget plus the wait for the regulatory domain.
const AP_WAIT: Duration = Duration::from_secs(30);
const AP_POLL: Duration = Duration::from_millis(250);

async fn on_air(cp: &CpConfig) -> Option<OnAir> {
    match cp.ap_on_air {
        Some(ask) => tokio::task::spawn_blocking(ask).await.ok().flatten(),
        None => livi_wifi::on_air(&cp.wifi_iface),
    }
}

/// The answer names an SSID and a channel, so it waits until they are on air and returns them.
async fn wait_for_ap(cp: &CpConfig) -> Option<OnAir> {
    let iface = &cp.wifi_iface;
    let remote = cp.ap_on_air.is_some();
    if !remote && (iface.is_empty() || !Path::new(&format!("/sys/class/net/{iface}")).exists()) {
        return None;
    }
    // A remote access point may still run what it had before ours was set.
    let up = |live: &Option<OnAir>| live.as_ref().is_some_and(|ap| !remote || ap.ssid == cp.ssid);
    let mut live = on_air(cp).await;
    if up(&live) {
        return live;
    }
    let what = if remote {
        "the dongle's access point".to_string()
    } else {
        format!("the access point on {iface}")
    };
    println!("[cp] waiting for {what}");
    let deadline = Instant::now() + AP_WAIT;
    while Instant::now() < deadline {
        tokio::time::sleep(AP_POLL).await;
        live = on_air(cp).await;
        if up(&live) {
            if let Some(ap) = &live {
                println!("[cp] access point up: {} on channel {}", ap.ssid, ap.channel);
            }
            return live;
        }
    }
    println!("[cp] {what} stayed down, answering anyway");
    live
}

/// How long the phone's USB network function is waited for once the phone asks for CarPlay.
const USB_IFACE_WAIT: Duration = Duration::from_secs(5);

/// The A/V interface found after the session started, if it shows up in time.
async fn late_av_iface(cp: &CpConfig) -> Option<String> {
    let mut late = cp.av_iface_late.clone()?;
    let found =
        tokio::time::timeout(USB_IFACE_WAIT, late.wait_for(Option::is_some)).await.ok()?.ok()?;
    (*found).clone()
}

/// Runs the accessory side of a wireless CarPlay session: identification, MFi auth,
/// subscriptions, then the request/response phase (Wi-Fi config, CarPlayStartSession),
/// emitting progress and every subsequent incoming message id over `events`.
pub async fn run_accessory<C: ControlChannel, A: AsyncAuth>(
    mut ch: C,
    mut auth: A,
    id: Identity,
    mut cp: CpConfig,
    events: mpsc::Sender<BringupEvent>,
    mut vehicle: VehicleFeed,
) {
    if let Err(e) = run_identification(&mut ch, &id, cp.transport).await {
        let _ = events.send(BringupEvent::Failed(e.to_string())).await;
        return;
    }
    let _ = events.send(BringupEvent::Identified).await;

    if let Err(e) = run_auth(&mut ch, &mut auth).await {
        let _ = events.send(BringupEvent::Failed(e.to_string())).await;
        return;
    }
    let _ = events.send(BringupEvent::Authenticated).await;

    if cp.transport == Transport::Wired {
        let power = PowerSourceUpdate {
            available_current_for_device: Some(cp.available_current_ma),
            device_battery_should_charge_if_power_is_present: Some(true),
        };
        if ch.send(power.encode()).await.is_err() {
            let _ = events.send(BringupEvent::Closed).await;
            return;
        }
    }

    for sub in subscriptions() {
        if ch.send(sub).await.is_err() {
            let _ = events.send(BringupEvent::Closed).await;
            return;
        }
    }
    let _ = events.send(BringupEvent::Subscribed).await;

    let mut location_types = LocationTypes::default();
    let mut status_wanted = false;
    // Set once the phone offered CarPlay. A start asked for before that is dropped, the first one
    // still goes out when the offer comes.
    let mut offered = false;
    let mut phone_bt_mac: Option<String> = None;
    let again = cp.start_again.clone();
    loop {
        let frame = tokio::select! {
            frame = ch.recv() => match frame {
                Some(frame) => frame,
                None => break,
            },
            _ = asked(again.as_deref()) => {
                if !offered {
                    continue;
                }
                println!("[cp] the phone hears the start once more");
                if !send_start(&mut ch, &cp, None, &events, None).await {
                    break;
                }
                continue;
            }
            changed = vehicle.location.changed() => {
                if changed.is_err() {
                    continue;
                }
                let nmea = vehicle.location.borrow_and_update().1.clone();
                if !send_location(&mut ch, &location_types, &nmea).await {
                    break;
                }
                continue;
            }
            changed = vehicle.status.changed() => {
                if changed.is_err() {
                    continue;
                }
                let status = vehicle.status.borrow_and_update().clone();
                if status_wanted && !send_status(&mut ch, &status, id.fuels).await {
                    break;
                }
                continue;
            }
            changed = vehicle.seek.changed() => {
                if changed.is_err() {
                    continue;
                }
                let seek = vehicle.seek.borrow_and_update().1.clone();
                if !seek.meant_for(phone_bt_mac.as_deref()) {
                    continue;
                }
                println!("[cp] now playing: jump to {} ms", seek.ms);
                let jump = SetNowPlayingInformation { elapsed_ms: Some(seek.ms) };
                if ch.send(jump.encode()).await.is_err() {
                    break;
                }
                continue;
            }
        };
        let Some(msg_id) = frame_msg_id(&frame) else {
            continue;
        };
        match msg_id {
            0x4E0E => {
                if let Ok(m) = DeviceTransportIdentifierNotification::decode(&frame)
                    && !m.bluetooth_transport_id.is_empty()
                {
                    phone_bt_mac = Some(m.bluetooth_transport_id);
                }
            }
            0xFFFA => {
                location_types = StartLocationInformation::decode(&frame)
                    .map(|req| LocationTypes::from_request(&req))
                    .unwrap_or_default();
                println!("[cp] location: subscribed {:?}", location_types.names());
                // Only fixes from now on, not the one from before the phone asked.
                vehicle.location.mark_unchanged();
            }
            0xFFFC => {
                location_types = LocationTypes::default();
                println!("[cp] location: stopped");
            }
            0xA100 => {
                status_wanted = true;
                println!("[cp] vehicle status: subscribed");
                let status = vehicle.status.borrow_and_update().clone();
                if !send_status(&mut ch, &status, id.fuels).await {
                    break;
                }
            }
            0xA102 => {
                status_wanted = false;
                println!("[cp] vehicle status: stopped");
            }
            0x5702 => {
                let live = on_air(&cp).await;
                if ch.send(wifi_config(&cp, live.as_ref()).encode()).await.is_err() {
                    break;
                }
                let _ = events.send(BringupEvent::WifiConfigSent).await;
            }
            0x4300 => {
                if let Some(phone) = on_cable(&cp, &frame) {
                    println!("[cp] {phone} runs iAP2 over the cable, no start over Bluetooth");
                    break;
                }
                if cp.transport == Transport::Wired && cp.av_iface.is_none() {
                    cp.av_iface = late_av_iface(&cp).await;
                }
                let live =
                    if cp.transport == Transport::Wired { None } else { wait_for_ap(&cp).await };
                if !send_start(&mut ch, &cp, live, &events, Some(&frame)).await {
                    break;
                }
                offered = true;
            }
            _ => {}
        }
        if events.send(BringupEvent::Incoming { msg_id, frame }).await.is_err() {
            return;
        }
    }
    let _ = events.send(BringupEvent::Closed).await;
}

/// Resolves when the start is asked for again, never without a way to ask.
async fn asked(again: Option<&Notify>) {
    match again {
        Some(again) => again.notified().await,
        None => std::future::pending().await,
    }
}

/// Hands the phone a session start, answering `offer` when there is one. False once the channel
/// is gone.
async fn send_start<C: ControlChannel>(
    ch: &mut C,
    cp: &CpConfig,
    live: Option<OnAir>,
    events: &mpsc::Sender<BringupEvent>,
    offer: Option<&[u8]>,
) -> bool {
    let Some(start) = carplay_start_session(cp, live) else {
        let why = match (&cp.transport, cp.av_iface.as_deref()) {
            (Transport::Wired, None) => {
                "the phone's USB network interface was not found".to_string()
            }
            (Transport::Wired, Some(iface)) => format!("no link-local on {iface:?}"),
            _ => format!("no link-local on {:?}", cp.wifi_iface),
        };
        println!("[cp] CarPlayStartSession not sent: {why}");
        let _ = events.send(BringupEvent::Failed(why)).await;
        return true;
    };
    // Logs the phone's offer and our answer.
    match offer.map(CarPlayAvailability::decode) {
        Some(Ok(a)) => println!(
            "[cp] CarPlayAvailability wired={:?} wireless={:?}",
            a.wired_attributes, a.wireless_attributes
        ),
        Some(Err(e)) => println!("[cp] CarPlayAvailability undecodable: {e}"),
        None => {}
    }
    let ip = start
        .wireless_attributes
        .as_ref()
        .map(|w| &w.ip_address)
        .or_else(|| start.wired_attributes.as_ref().map(|w| &w.ip_address))
        .map(|a| a.join(","))
        .unwrap_or_default();
    println!(
        "[cp] CarPlayStartSession ip={ip} port={} device_id={} pk_len={}",
        start.port.unwrap_or(0),
        start.device_identifier.as_deref().unwrap_or("-"),
        start.public_key.as_deref().unwrap_or("").len()
    );
    if ch.send(start.encode()).await.is_err() {
        return false;
    }
    if let Some(id) = &start.device_identifier {
        crate::bonjour::named(id);
    }
    let _ = events.send(BringupEvent::CarPlayStartSent { ip }).await;
    true
}

/// Each sentence the phone asked for as its own message. False once the channel is gone.
async fn send_location<C: ControlChannel>(ch: &mut C, types: &LocationTypes, nmea: &str) -> bool {
    for line in types.wanted(nmea) {
        let msg = LocationInformation { nmea_sentence: line.to_string() };
        if ch.send(msg.encode()).await.is_err() {
            return false;
        }
    }
    true
}

async fn send_status<C: ControlChannel>(ch: &mut C, status: &VehicleStatus, fuels: Fuels) -> bool {
    if status.is_empty() {
        return true;
    }
    println!(
        "[cp] vehicle status → range={:?} temp={:?} warn={:?}",
        status.range, status.outside_temperature, status.range_warning
    );
    ch.send(status.update(fuels).encode()).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(ssid: &str, channel: u32) -> OnAir {
        OnAir { ssid: ssid.into(), channel: Channel::of_number(channel), security: Security::Wpa2 }
    }

    #[test]
    fn the_phone_is_told_the_network_on_air() {
        let cp = dongle_ap(|| None);
        let live = OnAir { security: Security::Wpa2Wpa3, ..ap("LIVI-Link", 149) };
        let told = wifi_config(&cp, Some(&live));
        assert_eq!(told.ssid.as_deref(), Some("LIVI-Link"));
        assert_eq!(told.channel, 149);
        assert_eq!(told.security_type, SecurityType::Wpa3Transition);
        let fallback = wifi_config(&cp, None);
        assert_eq!((fallback.ssid.as_deref(), fallback.channel), (Some("LIVI"), 36));
        assert_eq!(fallback.security_type, SecurityType::WpaWpa2);
    }

    #[test]
    fn every_security_has_its_iap2_number() {
        assert_eq!(security_type(Security::Wpa2) as u8, 2);
        assert_eq!(security_type(Security::Wpa2Wpa3) as u8, 3);
        assert_eq!(security_type(Security::Wpa3) as u8, 4);
    }

    fn dongle_ap(ask: AskOnAir) -> CpConfig {
        CpConfig {
            wifi_iface: "usb0".into(),
            ssid: "LIVI".into(),
            passphrase: "12345678".into(),
            channel: Channel::of_number(36),
            airplay_port: 7000,
            source_version: "950.7.1".into(),
            public_key: String::new(),
            transport: Transport::Wireless,
            av_iface: None,
            av_iface_late: None,
            available_current_ma: 500,
            ap_mac: None,
            ap_on_air: Some(ask),
            on_cable: None,
            start_again: None,
        }
    }

    #[test]
    fn a_bluetooth_session_gives_way_to_the_cable() {
        let offer = |phone: Option<&str>| {
            CarPlayAvailability {
                wired_attributes: None,
                wireless_attributes: Some(CarPlayAvailabilityWirelessAttributes {
                    available: Some(true),
                    bluetooth_transport_identifier: phone.map(str::to_string),
                }),
            }
            .encode()
        };
        let cabled = OnCable(Arc::new(|mac: &str| mac == "0c:6a:c4:4e:f3:2a"));
        let cp = CpConfig { on_cable: Some(cabled), ..dongle_ap(|| None) };
        assert_eq!(
            on_cable(&cp, &offer(Some("0c:6a:c4:4e:f3:2a"))).as_deref(),
            Some("0c:6a:c4:4e:f3:2a")
        );
        assert_eq!(on_cable(&cp, &offer(Some("aa:bb:cc:dd:ee:ff"))), None);
        assert_eq!(on_cable(&cp, &offer(None)), None);
        assert_eq!(on_cable(&dongle_ap(|| None), &offer(Some("0c:6a:c4:4e:f3:2a"))), None);
    }

    #[tokio::test]
    async fn a_dongle_carrying_our_ssid_is_not_waited_for() {
        let live = wait_for_ap(&dongle_ap(|| Some(ap("LIVI", 6)))).await;
        assert_eq!(live, Some(ap("LIVI", 6)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_dongle_on_another_ssid_is_answered_with_what_it_carries() {
        let live = wait_for_ap(&dongle_ap(|| Some(ap("old", 36)))).await;
        assert_eq!(live, Some(ap("old", 36)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_dongle_leaves_our_own_values() {
        let live = wait_for_ap(&dongle_ap(|| None)).await;
        assert_eq!(live, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_usb_interface_found_after_the_start_is_waited_for() {
        let (found, late) = watch::channel(None);
        let cp = CpConfig { av_iface_late: Some(late), ..dongle_ap(|| None) };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = found.send(Some("en13".into()));
        });
        assert_eq!(late_av_iface(&cp).await.as_deref(), Some("en13"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_usb_interface_that_never_shows_is_given_up_on() {
        let (_found, late) = watch::channel(None);
        let cp = CpConfig { av_iface_late: Some(late), ..dongle_ap(|| None) };
        assert_eq!(late_av_iface(&cp).await, None);
        assert_eq!(late_av_iface(&dongle_ap(|| None)).await, None);
    }
}
