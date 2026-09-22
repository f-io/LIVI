// Wireless Projection Protocol: the Android Auto Wi-Fi bootstrap, framed as a 4-byte
// header (u16 length, u16 message id) plus a protobuf body.

pub const MSG_WIFI_START_REQUEST: u16 = 1;
pub const MSG_WIFI_INFO_REQUEST: u16 = 2;
pub const MSG_WIFI_INFO_RESPONSE: u16 = 3;
pub const MSG_WIFI_VERSION_REQUEST: u16 = 4;
pub const MSG_WIFI_VERSION_RESPONSE: u16 = 5;
pub const MSG_WIFI_CONNECTION_STATUS: u16 = 6;
pub const MSG_WIFI_START_RESPONSE: u16 = 7;
pub const MSG_PING: u16 = 8;
pub const MSG_PONG: u16 = 9;

const SECURITY_WPA2_PERSONAL: u64 = 8;
const ACCESS_POINT_STATIC: u64 = 0;

use crate::proto::{pb_string, pb_varint, read_varint, varint};

pub fn frame(msg_id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(&msg_id.to_be_bytes());
    out.extend_from_slice(body);
    out
}

pub fn channel_to_freq_mhz(channel: u16) -> u64 {
    match channel {
        1..=13 => 2412 + (channel as u64 - 1) * 5,
        14 => 2484,
        36..=177 => 5180 + (channel as u64 - 36) * 5,
        _ => 5180,
    }
}

/// Announces our WPP version and the frequency the access point runs on.
pub fn wifi_version_request(channel: u16) -> Vec<u8> {
    let freq_body = varint(channel_to_freq_mhz(channel));
    let mut body = pb_varint(1, 6);
    body.extend(pb_varint(2, 0));
    // Field 4, packed: the list of supported channel frequencies.
    body.push(0x22);
    body.extend(varint(freq_body.len() as u64));
    body.extend(freq_body);
    frame(MSG_WIFI_VERSION_REQUEST, &body)
}

/// Tells the phone where the projection listener waits once it has joined the AP.
pub fn wifi_start_request(ip: &str, port: u16) -> Vec<u8> {
    let mut body = pb_string(1, ip);
    body.extend(pb_varint(2, port as u64));
    frame(MSG_WIFI_START_REQUEST, &body)
}

/// The access point credentials the phone asked for.
pub fn wifi_info_response(ssid: &str, key: &str, bssid: &str) -> Vec<u8> {
    let mut body = pb_string(1, ssid);
    body.extend(pb_string(2, key));
    body.extend(pb_string(3, bssid));
    body.extend(pb_varint(4, SECURITY_WPA2_PERSONAL));
    body.extend(pb_varint(5, ACCESS_POINT_STATIC));
    frame(MSG_WIFI_INFO_RESPONSE, &body)
}

pub fn pong(body: &[u8]) -> Vec<u8> {
    frame(MSG_PONG, body)
}

/// The phone's identity from a WifiVersionResponse: field 3 is the serial (same as the USB
/// descriptor serial), field 6 → field 1 is the instance id.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub instance_id: String,
    pub serial: String,
}

pub fn parse_identity(data: &[u8]) -> Identity {
    let mut id = Identity::default();
    let mut i = 0usize;
    while i < data.len() {
        let Some(tag) = read_varint(data, &mut i) else { break };
        let field = tag >> 3;
        match tag & 7 {
            2 => {
                let Some(len) = read_varint(data, &mut i) else { break };
                let end = i + len as usize;
                let Some(val) = data.get(i..end) else { break };
                i = end;
                if field == 3 {
                    id.serial = String::from_utf8_lossy(val).into_owned();
                } else if field == 6 {
                    let mut j = 0usize;
                    while j < val.len() {
                        let Some(t2) = read_varint(val, &mut j) else { break };
                        match t2 & 7 {
                            2 => {
                                let Some(l2) = read_varint(val, &mut j) else { break };
                                let e2 = j + l2 as usize;
                                let Some(v2) = val.get(j..e2) else { break };
                                j = e2;
                                if t2 >> 3 == 1 {
                                    id.instance_id = String::from_utf8_lossy(v2).into_owned();
                                }
                            }
                            0 => {
                                if read_varint(val, &mut j).is_none() {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
            }
            0 => {
                if read_varint(data, &mut i).is_none() {
                    break;
                }
            }
            _ => break,
        }
    }
    id
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
        let f = wifi_version_request(36);
        assert_eq!(&f[..4], &[0x00, 0x08, 0x00, 0x04]);
        assert_eq!(&f[4..], &[0x08, 0x06, 0x10, 0x00, 0x22, 0x02, 0xBC, 0x28]);
    }

    #[test]
    fn frequencies() {
        assert_eq!(channel_to_freq_mhz(1), 2412);
        assert_eq!(channel_to_freq_mhz(11), 2462);
        assert_eq!(channel_to_freq_mhz(14), 2484);
        assert_eq!(channel_to_freq_mhz(36), 5180);
        assert_eq!(channel_to_freq_mhz(149), 5745);
    }

    #[test]
    fn start_request_carries_endpoint() {
        let f = wifi_start_request("10.10.0.1", 5277);
        assert_eq!(u16::from_be_bytes([f[2], f[3]]), MSG_WIFI_START_REQUEST);
        assert!(f.windows(9).any(|w| w == b"10.10.0.1"));
    }

    #[test]
    fn info_response_carries_credentials() {
        let f = wifi_info_response("LIVI-cm5", "secret123", "2c:cf:67:ee:c1:e0");
        assert_eq!(u16::from_be_bytes([f[2], f[3]]), MSG_WIFI_INFO_RESPONSE);
        assert!(f.windows(8).any(|w| w == b"LIVI-cm5"));
        assert!(f.windows(9).any(|w| w == b"secret123"));
    }

    #[test]
    fn identity_from_version_response() {
        let mut body = pb_string(3, "SERIAL123");
        let inner = pb_string(1, "instance-xyz");
        body.extend(varint(((6 << 3) | 2) as u64));
        body.extend(varint(inner.len() as u64));
        body.extend(inner);
        let id = parse_identity(&body);
        assert_eq!(id.serial, "SERIAL123");
        assert_eq!(id.instance_id, "instance-xyz");
    }

    #[test]
    fn identity_tolerates_garbage() {
        assert_eq!(parse_identity(&[0xFF, 0xFF, 0xFF]), Identity::default());
    }

    #[test]
    fn frame_reader_splits_stream() {
        let mut r = FrameReader::default();
        r.push(&wifi_start_request("1.2.3.4", 5277));
        r.push(&frame(MSG_PING, &[0xAA]));
        assert_eq!(r.next_frame().unwrap().0, MSG_WIFI_START_REQUEST);
        assert_eq!(r.next_frame().unwrap(), (MSG_PING, vec![0xAA]));
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
