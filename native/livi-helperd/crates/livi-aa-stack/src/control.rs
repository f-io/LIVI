use livi_aa_proto::{
    AudioFocusNotification, AudioFocusRequest, BatteryStatusNotification, ByeByeRequest,
    ByeByeResponse, ChannelOpenRequest, ChannelOpenResponse, MessageStatus, PingRequest,
    PingResponse, ServiceDiscoveryRequest, VoiceSessionNotification,
};

use crate::channels::{Emit, Frame};
use crate::codec::{decode, encode};
use crate::consts::{ch, ctrl_msg, frame_flags, media_msg};
use crate::log::{debug, detail, hex};

const OK: i32 = MessageStatus::Success as i32;

#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    ServiceDiscoveryRequest(ServiceDiscoveryRequest),
    ChannelOpenRequest(i32),
    AvSetupRequest {
        ch: u8,
        payload: Vec<u8>,
    },
    Ping(i64),
    Pong,
    /// 0 when the focus type could not be read.
    AudioFocusRequest(u8),
    Battery {
        level: Option<u32>,
        critical: bool,
        time_remaining_s: Option<u32>,
    },
    /// Already answered.
    Shutdown(i32),
    /// The phone answered our goodbye.
    ShutdownComplete,
    VoiceSession(bool),
}

pub type ControlOut = Emit<ControlEvent>;

fn send(out: &mut Vec<ControlOut>, flags: u8, msg_id: u16, payload: Vec<u8>) {
    out.push(Emit::Send(Frame::new(ch::CONTROL, flags, msg_id, payload)));
}

/// Focus types 1 gain, 2 and 3 transient, 4 release. States 1 gain,
/// 2 transient, 3 loss.
pub fn audio_focus_state(focus_type: u8) -> u8 {
    match focus_type {
        1 => 1,
        2 | 3 => 2,
        _ => 3,
    }
}

#[derive(Debug, Default)]
pub struct ControlChannel;

impl ControlChannel {
    pub fn handle_message(&mut self, msg_id: u16, payload: &[u8]) -> Vec<ControlOut> {
        let mut out = Vec::new();
        match msg_id {
            ctrl_msg::SERVICE_DISCOVERY_REQUEST => {
                let req = match decode::<ServiceDiscoveryRequest>(payload, &[]) {
                    Ok(req) => {
                        println!(
                            "[ControlChannel] ServiceDiscoveryRequest label=\"{}\" phone=\"{}\" device_id=\"{}\"",
                            req.head_unit_label.as_deref().unwrap_or("?"),
                            req.phone_make_and_model.as_deref().unwrap_or("?"),
                            req.mobile_device_identity
                                .as_ref()
                                .and_then(|p| p.mobile_device_id.as_deref())
                                .unwrap_or("")
                        );
                        req
                    }
                    Err(e) => {
                        eprintln!("[ControlChannel] failed to parse ServiceDiscoveryRequest: {e}");
                        ServiceDiscoveryRequest::default()
                    }
                };
                out.push(Emit::Event(ControlEvent::ServiceDiscoveryRequest(req)));
            }
            ctrl_msg::CHANNEL_OPEN_RESPONSE => {
                if let Ok(resp) = decode::<ChannelOpenResponse>(payload, &[1])
                    && resp.status != OK
                {
                    eprintln!("[ControlChannel] ChannelOpenResponse status={}", resp.status);
                }
            }
            ctrl_msg::CHANNEL_OPEN_REQUEST => {
                if let Ok(req) = decode::<ChannelOpenRequest>(payload, &[1, 2]) {
                    out.push(Emit::Event(ControlEvent::ChannelOpenRequest(req.service_id)));
                }
            }
            ctrl_msg::PING_REQUEST => match decode::<PingRequest>(payload, &[1]) {
                Ok(req) => {
                    let resp =
                        encode(&PingResponse { timestamp_ns: req.timestamp_ns, payload: None });
                    send(&mut out, frame_flags::ENC_SIGNAL, ctrl_msg::PING_RESPONSE, resp);
                    out.push(Emit::Event(ControlEvent::Ping(req.timestamp_ns)));
                }
                Err(e) => eprintln!("[ControlChannel] ping parse error: {e}"),
            },
            ctrl_msg::PING_RESPONSE => out.push(Emit::Event(ControlEvent::Pong)),
            ctrl_msg::AUDIO_FOCUS_REQUEST => self.on_audio_focus_request(payload, &mut out),
            ctrl_msg::BATTERY_STATUS_NOTIFICATION => {
                match decode::<BatteryStatusNotification>(payload, &[1]) {
                    Ok(b) => {
                        let critical = b.in_low_power_mode == Some(true);
                        detail!(
                            "[ControlChannel] battery {}% critical={critical}",
                            b.battery_level
                        );
                        out.push(Emit::Event(ControlEvent::Battery {
                            level: Some(b.battery_level),
                            critical,
                            time_remaining_s: b.time_remaining_seconds,
                        }));
                    }
                    Err(e) => eprintln!("[ControlChannel] battery parse error: {e}"),
                }
            }
            ctrl_msg::NAV_FOCUS_REQUEST => {
                if debug() {
                    println!("[ControlChannel] NavigationFocusRequest raw: {}", hex(payload));
                }
                send(
                    &mut out,
                    frame_flags::ENC_SIGNAL,
                    ctrl_msg::NAV_FOCUS_NOTIFICATION,
                    payload.to_vec(),
                );
            }
            ctrl_msg::BYEBYE_REQUEST => {
                let reason = match decode::<ByeByeRequest>(payload, &[1]) {
                    Ok(req) => req.reason,
                    Err(e) => {
                        eprintln!("[ControlChannel] ByeByeRequest parse error: {e}");
                        0
                    }
                };
                println!("[ControlChannel] ByeByeRequest reason={reason}, answering");
                send(
                    &mut out,
                    frame_flags::ENC_SIGNAL,
                    ctrl_msg::BYEBYE_RESPONSE,
                    encode(&ByeByeResponse {}),
                );
                out.push(Emit::Event(ControlEvent::Shutdown(reason)));
            }
            ctrl_msg::BYEBYE_RESPONSE => out.push(Emit::Event(ControlEvent::ShutdownComplete)),
            ctrl_msg::VOICE_SESSION_NOTIFICATION => {
                let status = decode::<VoiceSessionNotification>(payload, &[])
                    .ok()
                    .and_then(|v| v.status)
                    .unwrap_or(0);
                let name = match status {
                    1 => "START".to_string(),
                    2 => "END".to_string(),
                    s => format!("?({s})"),
                };
                detail!("[ControlChannel] VoiceSessionNotification status={name}");
                out.push(Emit::Event(ControlEvent::VoiceSession(status == 1)));
            }
            media_msg::SETUP => {
                if debug() {
                    println!("[ControlChannel] SETUP on the control channel, ignored");
                }
            }
            other => {
                if debug() {
                    println!("[ControlChannel] unhandled msgId=0x{other:x} len={}", payload.len());
                }
            }
        }
        out
    }

    pub fn handle_av_setup_request(&mut self, ch: u8, payload: &[u8]) -> Vec<ControlOut> {
        vec![Emit::Event(ControlEvent::AvSetupRequest { ch, payload: payload.to_vec() })]
    }

    pub fn channel_open_response(&self, status: i32) -> Frame {
        Frame::new(
            ch::CONTROL,
            frame_flags::ENC_CONTROL,
            ctrl_msg::CHANNEL_OPEN_RESPONSE,
            encode(&ChannelOpenResponse { status }),
        )
    }

    fn on_audio_focus_request(&mut self, payload: &[u8], out: &mut Vec<ControlOut>) {
        if debug() {
            println!("[ControlChannel] AudioFocusRequest raw: {}", hex(payload));
        }
        let focus_type =
            decode::<AudioFocusRequest>(payload, &[1]).map_or(0, |r| r.request_type as u8);
        let state = audio_focus_state(focus_type);
        let type_name = match focus_type {
            1 => "GAIN",
            2 => "GAIN_TRANSIENT",
            3 => "GAIN_TRANSIENT_MAY_DUCK",
            4 => "RELEASE",
            _ => "?",
        };
        let state_name = match state {
            1 => "GAIN",
            2 => "GAIN_TRANSIENT",
            _ => "LOSS",
        };
        println!(
            "[ControlChannel] AudioFocus type={focus_type}({type_name}) -> state={state}({state_name})"
        );
        let notification =
            encode(&AudioFocusNotification { focus_state: i32::from(state), unsolicited: None });
        send(out, frame_flags::ENC_SIGNAL, ctrl_msg::AUDIO_FOCUS_NOTIFICATION, notification);
        out.push(Emit::Event(ControlEvent::AudioFocusRequest(focus_type)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(out: &[ControlOut]) -> Vec<&Frame> {
        out.iter()
            .filter_map(|o| match o {
                Emit::Send(f) => Some(f),
                Emit::Event(_) => None,
            })
            .collect()
    }

    fn events(out: Vec<ControlOut>) -> Vec<ControlEvent> {
        out.into_iter()
            .filter_map(|o| match o {
                Emit::Event(e) => Some(e),
                Emit::Send(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_ping_is_answered_encrypted() {
        let mut c = ControlChannel;
        let out = c.handle_message(ctrl_msg::PING_REQUEST, &[0x08, 0xe8, 0x07]);
        assert_eq!(
            frames(&out),
            [&Frame::new(
                ch::CONTROL,
                frame_flags::ENC_SIGNAL,
                ctrl_msg::PING_RESPONSE,
                [0x08, 0xe8, 0x07]
            )]
        );
        assert_eq!(events(out), [ControlEvent::Ping(1000)]);
        assert!(c.handle_message(ctrl_msg::PING_REQUEST, &[0x10, 0x01]).is_empty());
        assert_eq!(events(c.handle_message(ctrl_msg::PING_RESPONSE, &[])), [ControlEvent::Pong]);
    }

    #[test]
    fn a_goodbye_is_read_from_its_field_and_answered() {
        let mut c = ControlChannel;
        let out = c.handle_message(ctrl_msg::BYEBYE_REQUEST, &[0x08, 0x02]);
        assert_eq!(
            frames(&out),
            [&Frame::new(ch::CONTROL, frame_flags::ENC_SIGNAL, ctrl_msg::BYEBYE_RESPONSE, [])]
        );
        assert_eq!(events(out), [ControlEvent::Shutdown(2)]);
        let out = c.handle_message(ctrl_msg::BYEBYE_REQUEST, &[]);
        assert_eq!(frames(&out).len(), 1);
        assert_eq!(events(out), [ControlEvent::Shutdown(0)]);
        assert_eq!(
            events(c.handle_message(ctrl_msg::BYEBYE_RESPONSE, &[])),
            [ControlEvent::ShutdownComplete]
        );
    }

    #[test]
    fn audio_focus_is_granted_by_type() {
        let mut c = ControlChannel;
        for (req, state) in [(1u8, 1u8), (2, 2), (3, 2), (4, 3), (9, 3)] {
            let out = c.handle_message(ctrl_msg::AUDIO_FOCUS_REQUEST, &[0x08, req]);
            assert_eq!(frames(&out)[0].payload, [0x08, state]);
            assert_eq!(events(out), [ControlEvent::AudioFocusRequest(req)]);
        }
        let out = c.handle_message(ctrl_msg::AUDIO_FOCUS_REQUEST, &[0x10, 0x01]);
        assert_eq!(frames(&out)[0].payload, [0x08, 3]);
        assert_eq!(events(out), [ControlEvent::AudioFocusRequest(0)]);
    }

    #[test]
    fn the_rest_of_the_control_messages() {
        let mut c = ControlChannel;
        let out = c.handle_message(ctrl_msg::NAV_FOCUS_REQUEST, &[0x08, 0x02]);
        assert_eq!(frames(&out)[0].msg_id, ctrl_msg::NAV_FOCUS_NOTIFICATION);
        assert_eq!(frames(&out)[0].payload, [0x08, 0x02]);
        assert_eq!(
            events(c.handle_message(ctrl_msg::VOICE_SESSION_NOTIFICATION, &[0x08, 0x01])),
            [ControlEvent::VoiceSession(true)]
        );
        assert_eq!(
            events(c.handle_message(ctrl_msg::VOICE_SESSION_NOTIFICATION, &[])),
            [ControlEvent::VoiceSession(false)]
        );
        assert_eq!(
            events(
                c.handle_message(ctrl_msg::BATTERY_STATUS_NOTIFICATION, &[0x08, 0x50, 0x18, 0x01])
            ),
            [ControlEvent::Battery { level: Some(80), critical: true, time_remaining_s: None }]
        );
        assert!(c.handle_message(ctrl_msg::BATTERY_STATUS_NOTIFICATION, &[0x18, 0x01]).is_empty());
        assert_eq!(
            events(c.handle_message(ctrl_msg::CHANNEL_OPEN_REQUEST, &[0x08, 0x00, 0x10, 0x03])),
            [ControlEvent::ChannelOpenRequest(3)]
        );
        assert!(c.handle_message(ctrl_msg::CHANNEL_OPEN_REQUEST, &[0x10, 0x03]).is_empty());
        assert!(c.handle_message(ctrl_msg::CHANNEL_OPEN_RESPONSE, &[0x08, 0x01]).is_empty());
        assert!(c.handle_message(media_msg::SETUP, &[]).is_empty());
        assert!(c.handle_message(0x7777, &[]).is_empty());
        let sdr = events(
            c.handle_message(ctrl_msg::SERVICE_DISCOVERY_REQUEST, &[0x22, 0x02, b'P', b'x']),
        );
        assert_eq!(
            sdr,
            [ControlEvent::ServiceDiscoveryRequest(ServiceDiscoveryRequest {
                head_unit_label: Some("Px".into()),
                ..Default::default()
            })]
        );
        let broken = events(c.handle_message(ctrl_msg::SERVICE_DISCOVERY_REQUEST, &[0x22, 0x09]));
        assert_eq!(broken, [ControlEvent::ServiceDiscoveryRequest(Default::default())]);
        assert_eq!(
            c.handle_av_setup_request(4, &[1]),
            [Emit::Event(ControlEvent::AvSetupRequest { ch: 4, payload: vec![1] })]
        );
        assert_eq!(
            c.channel_open_response(OK),
            Frame::new(
                ch::CONTROL,
                frame_flags::ENC_CONTROL,
                ctrl_msg::CHANNEL_OPEN_RESPONSE,
                [0x08, 0x00]
            )
        );
    }
}
