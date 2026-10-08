// Wireless Projection Protocol: the Android Auto Wi-Fi bootstrap, framed as a 4-byte
// header (u16 length, u16 message id) plus a protobuf body.

use livi_aa_proto::{
    WifiAccessPointType, WifiConnectStatusNotification, WifiInfoResponse, WifiSecurityMode,
    WifiStartRequest, WifiVersionRequest, WifiVersionResponse, WirelessSetupMessageId as Id,
};
use livi_wifi::{Channel, Security};
use prost::Message;

pub const MSG_WIFI_START_REQUEST: u16 = Id::WifiRequestStartBt as u16;
pub const MSG_WIFI_INFO_REQUEST: u16 = Id::WifiRequestInfoBt as u16;
pub const MSG_WIFI_INFO_RESPONSE: u16 = Id::WifiResponseInfoBt as u16;
pub const MSG_WIFI_VERSION_REQUEST: u16 = Id::WifiVersionRequestBt as u16;
pub const MSG_WIFI_VERSION_RESPONSE: u16 = Id::WifiVersionResponseBt as u16;
pub const MSG_WIFI_CONNECT_STATUS: u16 = Id::WifiConnectStatusBt as u16;
pub const MSG_WIFI_START_RESPONSE: u16 = Id::WifiResponseStartBt as u16;
pub const MSG_WIFI_PING_REQUEST: u16 = Id::WifiPingRequestBt as u16;
pub const MSG_WIFI_PING_RESPONSE: u16 = Id::WifiPingResponseBt as u16;

/// The phone refuses WPA3 alone and picks WPA3 out of the transition itself when it runs SAE,
/// so a WPA3-only access point is told as the transition.
fn security_mode(security: Security) -> WifiSecurityMode {
    match security {
        Security::Wpa2 => WifiSecurityMode::Wpa2Personal,
        Security::Wpa2Wpa3 | Security::Wpa3 => WifiSecurityMode::Wpa2Wpa3Personal,
    }
}

pub fn frame(msg_id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// Announces our WPP version and the frequency the access point runs on.
pub fn wifi_version_request(channel: Channel) -> Vec<u8> {
    let request = WifiVersionRequest {
        major_version: Some(6),
        minor_version: Some(0),
        supported_channel_frequencies_mhz: vec![channel.freq_mhz() as i32],
        ..Default::default()
    };
    frame(MSG_WIFI_VERSION_REQUEST, &request.encode_to_vec())
}

/// Tells the phone where the projection listener waits once it has joined the AP.
pub fn wifi_start_request(ip: &str, port: u16) -> Vec<u8> {
    let request = WifiStartRequest {
        ip_address: Some(ip.to_string()),
        port: Some(i32::from(port)),
        reason: None,
    };
    frame(MSG_WIFI_START_REQUEST, &request.encode_to_vec())
}

/// The access point credentials the phone asked for.
pub fn wifi_info_response(ssid: &str, key: &str, bssid: &str, security: Security) -> Vec<u8> {
    let response = WifiInfoResponse {
        ssid: Some(ssid.to_string()),
        password: Some(key.to_string()),
        bssid: Some(bssid.to_string()),
        security_mode: Some(security_mode(security) as i32),
        access_point_type: Some(WifiAccessPointType::Static as i32),
    };
    frame(MSG_WIFI_INFO_RESPONSE, &response.encode_to_vec())
}

pub fn pong(body: &[u8]) -> Vec<u8> {
    frame(MSG_WIFI_PING_RESPONSE, body)
}

/// 0 when the phone joined the access point.
pub fn connect_status(body: &[u8]) -> i32 {
    WifiConnectStatusNotification::decode(body).ok().and_then(|s| s.status).unwrap_or(0)
}

/// The phone's identity from a WifiVersionResponse: the serial (same as the USB descriptor
/// serial) and the device id.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub instance_id: String,
    pub serial: String,
}

pub fn parse_identity(data: &[u8]) -> Identity {
    let Ok(response) = WifiVersionResponse::decode(data) else { return Identity::default() };
    Identity {
        instance_id: response
            .mobile_device_identity
            .and_then(|i| i.mobile_device_id)
            .unwrap_or_default(),
        serial: response.device_serial.unwrap_or_default(),
    }
}

/// Splits the RFCOMM byte stream into WPP frames.
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn next_frame(&mut self) -> Option<(u16, Vec<u8>)> {
        if self.buf.len() < 4 {
            return None;
        }
        let len = u16::from_be_bytes([self.buf[0], self.buf[1]]) as usize;
        let msg_id = u16::from_be_bytes([self.buf[2], self.buf[3]]);
        if self.buf.len() < 4 + len {
            return None;
        }
        let body = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Some((msg_id, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_request_matches_reference() {
        // Channel 36 → 5180 MHz, the shape the phone expects.
        let f = wifi_version_request(Channel::of_number(36));
        assert_eq!(&f[..4], &[0x00, 0x07, 0x00, 0x04]);
        assert_eq!(&f[4..], &[0x08, 0x06, 0x10, 0x00, 0x20, 0xBC, 0x28]);
    }

    #[test]
    fn a_6_ghz_channel_announces_its_own_frequency() {
        let f = wifi_version_request(Channel::new(livi_wifi::Band::Ghz6, 37));
        assert_eq!(&f[8..], &[0x20, 0xF7, 0x2F]);
    }

    #[test]
    fn the_security_goes_out_in_the_phones_numbering() {
        for (security, mode) in
            [(Security::Wpa2, 8), (Security::Wpa2Wpa3, 40), (Security::Wpa3, 40)]
        {
            let f = wifi_info_response("LIVI", "secret123", "aa:bb:cc:dd:ee:ff", security);
            assert!(f.ends_with(&[0x20, mode, 0x28, 0x00]), "{security}");
        }
    }

    #[test]
    fn start_request_carries_endpoint() {
        let f = wifi_start_request("10.10.0.1", 5277);
        assert_eq!(u16::from_be_bytes([f[2], f[3]]), MSG_WIFI_START_REQUEST);
        assert!(f.windows(9).any(|w| w == b"10.10.0.1"));
    }

    #[test]
    fn info_response_carries_credentials() {
        let f = wifi_info_response("LIVI-cm5", "secret123", "2c:cf:67:ee:c1:e0", Security::Wpa2);
        assert_eq!(u16::from_be_bytes([f[2], f[3]]), MSG_WIFI_INFO_RESPONSE);
        assert!(f.windows(8).any(|w| w == b"LIVI-cm5"));
        assert!(f.windows(9).any(|w| w == b"secret123"));
    }

    #[test]
    fn identity_from_version_response() {
        let response = WifiVersionResponse {
            device_serial: Some("SERIAL123".into()),
            mobile_device_identity: Some(livi_aa_proto::MobileDeviceIdentity {
                mobile_device_id: Some("instance-xyz".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let id = parse_identity(&response.encode_to_vec());
        assert_eq!(id.serial, "SERIAL123");
        assert_eq!(id.instance_id, "instance-xyz");
    }

    #[test]
    fn the_join_status_comes_out_of_its_field() {
        let failed = WifiConnectStatusNotification { status: Some(-3), error_message_hint: None };
        assert_eq!(connect_status(&failed.encode_to_vec()), -3);
        assert_eq!(connect_status(&[]), 0);
    }

    #[test]
    fn identity_tolerates_garbage() {
        assert_eq!(parse_identity(&[0xFF, 0xFF, 0xFF]), Identity::default());
    }

    #[test]
    fn frame_reader_splits_stream() {
        let mut r = FrameReader::default();
        r.push(&wifi_start_request("1.2.3.4", 5277));
        r.push(&frame(MSG_WIFI_PING_REQUEST, &[0xAA]));
        assert_eq!(r.next_frame().unwrap().0, MSG_WIFI_START_REQUEST);
        assert_eq!(r.next_frame().unwrap(), (MSG_WIFI_PING_REQUEST, vec![0xAA]));
        assert_eq!(r.next_frame(), None);
    }

    #[test]
    fn frame_reader_waits_for_body() {
        let mut r = FrameReader::default();
        r.push(&[0x00, 0x04, 0x00, 0x05, 0x01]);
        assert_eq!(r.next_frame(), None);
        r.push(&[0x02, 0x03, 0x04]);
        assert_eq!(r.next_frame().unwrap(), (5, vec![1, 2, 3, 4]));
    }
}
