// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum CpRequest {
    Subscribe,
    Tunnel { cid: String, bt_mac: Option<String> },
    Certificate,
    Sign(Vec<u8>),
    Disconnect(String),
    ReconnectTargets(Vec<(String, Option<String>)>),
    Location(String),
    VehicleStatus(Value),
    Seek { ms: u32, bt_mac: Option<String> },
    DropIap2,
    StartWired(String),
    ResetWired(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AaRequest {
    Subscribe,
    WiredPhones(Vec<String>),
    ScoSink(Option<(String, u32)>),
    RestartUsb(Option<String>),
}

fn verb_and_arg(line: &str) -> (&str, &str) {
    let line = line.trim_end_matches(['\r', '\n']);
    match line.split_once(' ') {
        Some((verb, arg)) => (verb, arg.trim()),
        None => (line, ""),
    }
}

fn unknown(verb: &str) -> String {
    format!("unknown command: {verb}")
}

impl fmt::Display for CpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscribe => f.write_str("subscribe"),
            Self::Tunnel { cid, bt_mac } => match bt_mac {
                Some(mac) => write!(f, "tunnel {cid} {mac}"),
                None => write!(f, "tunnel {cid}"),
            },
            Self::Certificate => f.write_str("certificate"),
            Self::Sign(digest) => write!(f, "sign {}", STANDARD.encode(digest)),
            Self::Disconnect(mac) => write!(f, "disconnect {mac}"),
            Self::ReconnectTargets(targets) => {
                let list: Vec<Value> = targets
                    .iter()
                    .map(|(mac, uuid)| Value::Array(vec![mac.clone().into(), uuid.clone().into()]))
                    .collect();
                write!(f, "reconnect-targets {}", Value::Array(list))
            }
            Self::Location(nmea) => write!(f, "location {}", STANDARD.encode(nmea)),
            Self::VehicleStatus(status) => write!(f, "vehicle-status {status}"),
            Self::Seek { ms, bt_mac } => match bt_mac {
                Some(mac) => write!(f, "seek {ms} {mac}"),
                None => write!(f, "seek {ms}"),
            },
            Self::DropIap2 => f.write_str("drop-iap2"),
            Self::StartWired(udid) => write!(f, "start-wired {udid}"),
            Self::ResetWired(udid) => write!(f, "reset-wired {udid}"),
        }
    }
}

fn reconnect_targets(arg: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let value: Value = serde_json::from_str(arg).map_err(|e| e.to_string())?;
    let list = value
        .as_array()
        .ok_or("reconnect-targets expects a JSON array")?;
    list.iter()
        .map(|pair| {
            let mac = pair
                .get(0)
                .and_then(Value::as_str)
                .ok_or("reconnect-targets entries are [mac, uuid] pairs")?;
            let uuid = pair.get(1).and_then(Value::as_str).map(str::to_string);
            Ok((mac.to_string(), uuid))
        })
        .collect()
}

impl CpRequest {
    pub fn parse(line: &str) -> Result<Self, String> {
        let (verb, arg) = verb_and_arg(line);
        Ok(match verb {
            "subscribe" => Self::Subscribe,
            "tunnel" => {
                let (cid, bt_mac) = match arg.rsplit_once(' ') {
                    Some((cid, mac)) if mac.contains(':') => (cid.trim(), Some(mac.to_string())),
                    _ => (arg, None),
                };
                Self::Tunnel {
                    cid: cid.to_string(),
                    bt_mac,
                }
            }
            "certificate" => Self::Certificate,
            "sign" => Self::Sign(
                STANDARD
                    .decode(arg)
                    .map_err(|e| format!("bad base64: {e}"))?,
            ),
            "disconnect" if arg.is_empty() => return Err("disconnect requires a MAC".into()),
            "disconnect" => Self::Disconnect(arg.to_string()),
            "reconnect-targets" => Self::ReconnectTargets(reconnect_targets(arg)?),
            "location" => {
                let bytes = STANDARD.decode(arg).map_err(|e| e.to_string())?;
                Self::Location(String::from_utf8_lossy(&bytes).into_owned())
            }
            "vehicle-status" => {
                Self::VehicleStatus(serde_json::from_str(arg).map_err(|e| e.to_string())?)
            }
            "seek" => {
                let mut words = arg.split_whitespace();
                let ms = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| format!("not a position in ms: {arg:?}"))?;
                Self::Seek {
                    ms,
                    bt_mac: words.next().map(str::to_string),
                }
            }
            "drop-iap2" => Self::DropIap2,
            "start-wired" => Self::StartWired(arg.to_string()),
            "reset-wired" => Self::ResetWired(arg.to_string()),
            other => return Err(unknown(other)),
        })
    }
}

impl fmt::Display for AaRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscribe => f.write_str("subscribe"),
            Self::WiredPhones(ids) => write!(f, "wired-phones {}", Value::from(ids.clone())),
            Self::ScoSink(Some((feed, stream))) => write!(f, "sco-sink {feed} {stream}"),
            Self::ScoSink(None) => f.write_str("sco-sink"),
            Self::RestartUsb(Some(serial)) => write!(f, "restart-usb {serial}"),
            Self::RestartUsb(None) => f.write_str("restart-usb"),
        }
    }
}

impl AaRequest {
    pub fn parse(line: &str) -> Result<Self, String> {
        let (verb, arg) = verb_and_arg(line);
        Ok(match verb {
            "subscribe" => Self::Subscribe,
            "wired-phones" if arg.is_empty() => Self::WiredPhones(Vec::new()),
            "wired-phones" => {
                Self::WiredPhones(serde_json::from_str(arg).map_err(|e| e.to_string())?)
            }
            "sco-sink" if arg.is_empty() => Self::ScoSink(None),
            "sco-sink" => {
                let (feed, stream) = arg
                    .split_once(' ')
                    .and_then(|(feed, id)| Some((feed, id.trim().parse::<u32>().ok()?)))
                    .ok_or_else(|| format!("sco-sink expects a feed and a stream: {arg:?}"))?;
                Self::ScoSink(Some((feed.to_string(), stream)))
            }
            "restart-usb" => Self::RestartUsb(Some(arg.to_string()).filter(|s| !s.is_empty())),
            other => return Err(unknown(other)),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn cp(request: CpRequest) {
        let line = request.to_string();
        assert_eq!(CpRequest::parse(&line), Ok(request), "{line}");
    }

    fn aa(request: AaRequest) {
        let line = request.to_string();
        assert_eq!(AaRequest::parse(&line), Ok(request), "{line}");
    }

    #[test]
    fn every_request_reads_back_what_it_wrote() {
        cp(CpRequest::Subscribe);
        cp(CpRequest::Tunnel {
            cid: "c1".into(),
            bt_mac: Some("AA:BB:CC:DD:EE:FF".into()),
        });
        cp(CpRequest::Tunnel {
            cid: "c1".into(),
            bt_mac: None,
        });
        cp(CpRequest::Certificate);
        cp(CpRequest::Sign(vec![0, 1, 254, 255]));
        cp(CpRequest::Disconnect("AA:BB".into()));
        cp(CpRequest::ReconnectTargets(vec![
            ("AA:BB".into(), Some("0000110a".into())),
            ("CC:DD".into(), None),
        ]));
        cp(CpRequest::ReconnectTargets(Vec::new()));
        cp(CpRequest::Location("$GPGGA,1*00\r\n".into()));
        cp(CpRequest::VehicleStatus(json!({ "range": 100 })));
        cp(CpRequest::Seek {
            ms: 60000,
            bt_mac: Some("AA:BB".into()),
        });
        cp(CpRequest::Seek {
            ms: 0,
            bt_mac: None,
        });
        cp(CpRequest::DropIap2);
        cp(CpRequest::StartWired("00008120".into()));
        cp(CpRequest::ResetWired("00008120".into()));

        aa(AaRequest::Subscribe);
        aa(AaRequest::WiredPhones(vec!["a".into(), "b".into()]));
        aa(AaRequest::WiredPhones(Vec::new()));
        aa(AaRequest::ScoSink(Some(("/tmp/feed".into(), 3))));
        aa(AaRequest::ScoSink(None));
        aa(AaRequest::RestartUsb(None));
        aa(AaRequest::RestartUsb(Some("R5CT".into())));
    }

    #[test]
    fn the_wire_text_stays_as_before() {
        assert_eq!(
            CpRequest::ReconnectTargets(vec![("AA".into(), None)]).to_string(),
            r#"reconnect-targets [["AA",null]]"#
        );
        assert_eq!(
            CpRequest::Location("$G".into()).to_string(),
            "location JEc="
        );
        assert_eq!(
            AaRequest::WiredPhones(vec!["a".into()]).to_string(),
            r#"wired-phones ["a"]"#
        );
        assert_eq!(
            CpRequest::parse("tunnel c1 "),
            Ok(CpRequest::Tunnel {
                cid: "c1".into(),
                bt_mac: None
            })
        );
        assert_eq!(
            AaRequest::parse("wired-phones"),
            Ok(AaRequest::WiredPhones(Vec::new()))
        );
        assert_eq!(
            CpRequest::parse("seek  60000  AA:BB:CC:DD:EE:FF "),
            Ok(CpRequest::Seek {
                ms: 60000,
                bt_mac: Some("AA:BB:CC:DD:EE:FF".into())
            })
        );
    }

    #[test]
    fn a_broken_request_is_refused_with_a_reason() {
        for (line, reason) in [
            ("bogus", "unknown command: bogus"),
            ("disconnect", "disconnect requires a MAC"),
            ("sign !!", "bad base64"),
            ("seek soon", "not a position in ms"),
            ("seek", "not a position in ms"),
            ("reconnect-targets {}", "expects a JSON array"),
            ("reconnect-targets [[1]]", "[mac, uuid] pairs"),
            ("vehicle-status {", "EOF"),
        ] {
            let err = CpRequest::parse(line).unwrap_err();
            assert!(err.contains(reason), "{line}: {err}");
        }
        for line in ["dance", "sco-sink /f", "sco-sink /f x", "wired-phones nope"] {
            assert!(AaRequest::parse(line).is_err(), "{line}");
        }
    }
}
