use tokio::sync::mpsc;

use iap2_csm::messages::authentication::*;
use iap2_csm::messages::car_play::*;
use iap2_csm::messages::communications::*;
use iap2_csm::messages::identification::*;
use iap2_csm::messages::now_playing::*;
use iap2_csm::messages::power::*;
use iap2_csm::messages::route_guidance::*;
use iap2_csm::messages::wifi::*;
use iap2_csm::CsmMessage;

use crate::framing::frame_msg_id;
use crate::ident::{build_identification, Identity, Transport, DROPPABLE};
use crate::{net, AsyncAuth, ControlChannel};

/// Wireless CarPlay parameters handed to the phone: the AP and the AirPlay receiver.
#[derive(Debug, Clone)]
pub struct CpConfig {
    pub wifi_iface: String,
    pub ssid: String,
    pub passphrase: String,
    pub channel: u8,
    pub security_type: SecurityType,
    pub airplay_port: u32,
    pub source_version: String,
    pub public_key: String,
    pub transport: Transport,
    /// Wired only: the USB network interface carrying the AV stream, whose link-local
    /// address the phone connects back to.
    pub av_iface: Option<String>,
    pub available_current_ma: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BringupEvent {
    Identified,
    Authenticated,
    Subscribed,
    WifiConfigSent,
    CarPlayStartSent,
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
                )))
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
            Some(0xAA04) => return Err(BringupError::Auth("device sent AuthenticationFailed".into())),
            other => {
                return Err(BringupError::Auth(format!("unexpected message during auth: {other:?}")))
            }
        }
    }
}

fn wifi_config(cp: &CpConfig) -> AccessoryWiFiConfigurationInformation {
    AccessoryWiFiConfigurationInformation {
        ssid: Some(cp.ssid.clone()),
        passphrase: Some(cp.passphrase.clone()),
        security_type: cp.security_type,
        channel: cp.channel,
    }
}

fn carplay_start_session(cp: &CpConfig) -> Option<CarPlayStartSession> {
    if cp.transport == Transport::Wired {
        // The link-local the phone connects to: the A/V interface's.
        let fe80 = net::wlan_link_local(cp.av_iface.as_deref()?)?;
        return Some(CarPlayStartSession {
            wired_attributes: Some(CarPlayStartSessionWiredAttributes { ip_address: vec![fe80] }),
            wireless_attributes: None,
            port: Some(cp.airplay_port),
            // No AP interface: the A/V interface stands in.
            device_identifier: net::wlan_mac(&cp.wifi_iface)
                .or_else(|| cp.av_iface.as_deref().and_then(net::wlan_mac)),
            public_key: Some(cp.public_key.clone()),
            source_version: Some(cp.source_version.clone()),
        });
    }
    let fe80 = net::wlan_link_local(&cp.wifi_iface)?;
    let (live_ssid, live_channel) = net::ap_ssid_channel(&cp.wifi_iface);
    let ssid = live_ssid.filter(|s| !s.is_empty()).unwrap_or_else(|| cp.ssid.clone());
    let channel = live_channel.filter(|c| *c != 0).unwrap_or(cp.channel);
    Some(CarPlayStartSession {
        wired_attributes: None,
        wireless_attributes: Some(CarPlayStartSessionWirelessAttributes {
            wifi_ssid: Some(ssid),
            passphrase: Some(cp.passphrase.clone()),
            channel: Some(channel),
            ip_address: vec![fe80],
            security_type: Some(cp.security_type as u8),
        }),
        port: Some(cp.airplay_port),
        device_identifier: net::wlan_mac(&cp.wifi_iface),
        public_key: Some(cp.public_key.clone()),
        source_version: Some(cp.source_version.clone()),
    })
}

/// Runs the accessory side of a wireless CarPlay session: identification, MFi auth,
/// subscriptions, then the request/response phase (Wi-Fi config, CarPlayStartSession),
/// emitting progress and every subsequent incoming message id over `events`.
pub async fn run_accessory<C: ControlChannel, A: AsyncAuth>(
    mut ch: C,
    mut auth: A,
    id: Identity,
    cp: CpConfig,
    events: mpsc::Sender<BringupEvent>,
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

    while let Some(frame) = ch.recv().await {
        let Some(msg_id) = frame_msg_id(&frame) else { continue };
        match msg_id {
            0x5702 => {
                if ch.send(wifi_config(&cp).encode()).await.is_err() {
                    break;
                }
                let _ = events.send(BringupEvent::WifiConfigSent).await;
            }
            0x4300 => match carplay_start_session(&cp) {
                Some(start) => {
                    // Logs the phone's offer and our answer.
                    match CarPlayAvailability::decode(&frame) {
                        Ok(a) => println!(
                            "[cp] CarPlayAvailability wired={:?} wireless={:?}",
                            a.wired_attributes, a.wireless_attributes
                        ),
                        Err(e) => println!("[cp] CarPlayAvailability undecodable: {e}"),
                    }
                    println!(
                        "[cp] CarPlayStartSession ip={:?} port={:?} device_id={:?} pk_len={}",
                        start.wired_attributes.as_ref().map(|w| &w.ip_address),
                        start.port,
                        start.device_identifier,
                        start.public_key.as_deref().unwrap_or("").len()
                    );
                    if ch.send(start.encode()).await.is_err() {
                        break;
                    }
                    let _ = events.send(BringupEvent::CarPlayStartSent).await;
                }
                None => {
                    let _ = events
                        .send(BringupEvent::Failed("no Wi-Fi link-local on AP interface".into()))
                        .await;
                }
            },
            _ => {}
        }
        if events.send(BringupEvent::Incoming { msg_id, frame }).await.is_err() {
            return;
        }
    }
    let _ = events.send(BringupEvent::Closed).await;
}
