//! The navigation status channel (12), phone to head unit only. Phones send the
//! turn and distance events, the state and position messages are handled for
//! those that use them.

use crate::channels::text;
use crate::consts::nav_msg;
use crate::log::detail;
use crate::wire::{WIRE_LEN, WIRE_VARINT, decode_fields, decode_varint_value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationState {
    Unavailable,
    Active,
    Inactive,
    Rerouting,
}

impl NavigationState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Rerouting => "rerouting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationTurnSide {
    Left,
    Right,
    Unspecified,
}

impl NavigationTurnSide {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Unspecified => "unspecified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationTurnEvent {
    Unknown,
    Depart,
    NameChange,
    SlightTurn,
    Turn,
    SharpTurn,
    UTurn,
    OnRamp,
    OffRamp,
    Fork,
    Merge,
    RoundaboutEnter,
    RoundaboutExit,
    RoundaboutEnterAndExit,
    Straight,
    FerryBoat,
    FerryTrain,
    Destination,
}

impl NavigationTurnEvent {
    pub const ALL: [Self; 18] = [
        Self::Unknown,
        Self::Depart,
        Self::NameChange,
        Self::SlightTurn,
        Self::Turn,
        Self::SharpTurn,
        Self::UTurn,
        Self::OnRamp,
        Self::OffRamp,
        Self::Fork,
        Self::Merge,
        Self::RoundaboutEnter,
        Self::RoundaboutExit,
        Self::RoundaboutEnterAndExit,
        Self::Straight,
        Self::FerryBoat,
        Self::FerryTrain,
        Self::Destination,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Depart => "depart",
            Self::NameChange => "name-change",
            Self::SlightTurn => "slight-turn",
            Self::Turn => "turn",
            Self::SharpTurn => "sharp-turn",
            Self::UTurn => "u-turn",
            Self::OnRamp => "on-ramp",
            Self::OffRamp => "off-ramp",
            Self::Fork => "fork",
            Self::Merge => "merge",
            Self::RoundaboutEnter => "roundabout-enter",
            Self::RoundaboutExit => "roundabout-exit",
            Self::RoundaboutEnterAndExit => "roundabout-enter-and-exit",
            Self::Straight => "straight",
            Self::FerryBoat => "ferry-boat",
            Self::FerryTrain => "ferry-train",
            Self::Destination => "destination",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationTurnUpdate {
    pub road: Option<String>,
    pub turn_side: Option<NavigationTurnSide>,
    pub event: Option<NavigationTurnEvent>,
    pub image: Option<Vec<u8>>,
    pub turn_number: Option<u32>,
    pub turn_angle: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationDistanceUpdate {
    pub distance_meters: u32,
    pub time_to_turn_seconds: u32,
    /// The shown value times 1000 in `display_unit`.
    pub display_distance_e3: Option<u32>,
    pub display_unit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationStateUpdate {
    pub maneuver_type: Option<u32>,
    pub road_name: Option<String>,
    pub cue: Option<String>,
    pub destination_address: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationPositionUpdate {
    pub step_distance_meters: Option<u32>,
    pub step_distance_display: Option<String>,
    pub time_to_step_seconds: Option<u32>,
    pub destination_meters: Option<u32>,
    pub destination_display: Option<String>,
    pub destination_units: Option<u32>,
    /// Arrival on the clock, "21:58".
    pub eta_text: Option<String>,
    pub time_to_arrival_seconds: Option<u32>,
    pub current_road_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationEvent {
    Start,
    Stop,
    Status(NavigationState),
    Turn(NavigationTurnUpdate),
    Distance(NavigationDistanceUpdate),
    State(NavigationStateUpdate),
    Position(NavigationPositionUpdate),
}

fn opt<T: std::fmt::Debug>(v: &Option<T>) -> String {
    v.as_ref().map_or_else(|| "undefined".to_string(), |v| format!("{v:?}"))
}

#[derive(Debug, Default)]
pub struct NavigationChannel;

impl NavigationChannel {
    pub fn handle_message(&mut self, msg_id: u16, payload: &[u8]) -> Option<NavigationEvent> {
        match msg_id {
            nav_msg::START => {
                println!("[NavigationChannel] START");
                Some(NavigationEvent::Start)
            }
            nav_msg::STOP => {
                println!("[NavigationChannel] STOP");
                Some(NavigationEvent::Stop)
            }
            nav_msg::STATUS => {
                let s = decode_status(payload);
                detail!("[NavigationChannel] status={s:?}");
                Some(NavigationEvent::Status(s))
            }
            nav_msg::NEXT_TURN => {
                let t = decode_turn_event(payload);
                detail!(
                    "[NavigationChannel] turn road={} event={} side={} angle={} image={}",
                    opt(&t.road),
                    opt(&t.event),
                    opt(&t.turn_side),
                    opt(&t.turn_angle),
                    t.image.as_ref().map_or("none".to_string(), |i| format!("{}B", i.len()))
                );
                Some(NavigationEvent::Turn(t))
            }
            nav_msg::NEXT_TURN_DISTANCE => {
                let d = decode_distance_event(payload);
                detail!(
                    "[NavigationChannel] distance {}m t={}s display={}/{}",
                    d.distance_meters,
                    d.time_to_turn_seconds,
                    opt(&d.display_distance_e3),
                    opt(&d.display_unit)
                );
                Some(NavigationEvent::Distance(d))
            }
            nav_msg::STATE => {
                let s = decode_state(payload);
                println!(
                    "[NavigationChannel] state maneuver={} road={} dest={}",
                    opt(&s.maneuver_type),
                    opt(&s.road_name),
                    opt(&s.destination_address)
                );
                Some(NavigationEvent::State(s))
            }
            nav_msg::CURRENT_POSITION => {
                let p = decode_position(payload);
                println!(
                    "[NavigationChannel] position dest={}m eta={} ttarr={}s step={}m",
                    opt(&p.destination_meters),
                    opt(&p.eta_text),
                    opt(&p.time_to_arrival_seconds),
                    opt(&p.step_distance_meters)
                );
                Some(NavigationEvent::Position(p))
            }
            other => {
                println!("[NavigationChannel] unhandled msgId=0x{other:x} len={}", payload.len());
                None
            }
        }
    }
}

pub fn decode_status(payload: &[u8]) -> NavigationState {
    let mut raw = 0;
    for f in decode_fields(payload) {
        if f.field == 1 && f.wire == WIRE_VARINT {
            raw = decode_varint_value(f.bytes);
        }
    }
    match raw {
        1 => NavigationState::Active,
        2 => NavigationState::Inactive,
        3 => NavigationState::Rerouting,
        _ => NavigationState::Unavailable,
    }
}

pub fn decode_turn_event(payload: &[u8]) -> NavigationTurnUpdate {
    let mut out = NavigationTurnUpdate::default();
    for f in decode_fields(payload) {
        match f.field {
            1 => out.road = Some(text(f.bytes)),
            2 => {
                out.turn_side = Some(match decode_varint_value(f.bytes) {
                    1 => NavigationTurnSide::Left,
                    2 => NavigationTurnSide::Right,
                    _ => NavigationTurnSide::Unspecified,
                })
            }
            3 => out.event = Some(next_turn(decode_varint_value(f.bytes))),
            4 => out.image = Some(f.bytes.to_vec()),
            5 => out.turn_number = Some(decode_varint_value(f.bytes)),
            6 => out.turn_angle = Some(decode_varint_value(f.bytes)),
            _ => {}
        }
    }
    out
}

pub fn decode_distance_event(payload: &[u8]) -> NavigationDistanceUpdate {
    let mut out = NavigationDistanceUpdate::default();
    for f in decode_fields(payload) {
        match f.field {
            1 => out.distance_meters = decode_varint_value(f.bytes),
            2 => out.time_to_turn_seconds = decode_varint_value(f.bytes),
            3 => out.display_distance_e3 = Some(decode_varint_value(f.bytes)),
            4 => out.display_unit = Some(decode_varint_value(f.bytes)),
            _ => {}
        }
    }
    out
}

pub fn decode_state(payload: &[u8]) -> NavigationStateUpdate {
    let mut out = NavigationStateUpdate::default();
    for f in decode_fields(payload) {
        let road_unset = out.road_name.as_deref().is_none_or(str::is_empty);
        if f.field == 1 && f.wire == WIRE_LEN && out.maneuver_type.is_none() && road_unset {
            for s in decode_fields(f.bytes) {
                if s.field == 1 && s.wire == WIRE_LEN {
                    for m in decode_fields(s.bytes) {
                        if m.field == 1 && m.wire == WIRE_VARINT {
                            out.maneuver_type = Some(decode_varint_value(m.bytes));
                        }
                    }
                } else if s.field == 2 && s.wire == WIRE_LEN {
                    for r in decode_fields(s.bytes) {
                        if r.field == 1 && r.wire == WIRE_LEN {
                            out.road_name = Some(text(r.bytes));
                        }
                    }
                } else if s.field == 4 && s.wire == WIRE_LEN && out.cue.is_none() {
                    for c in decode_fields(s.bytes) {
                        if c.field == 1 && c.wire == WIRE_LEN && out.cue.is_none() {
                            out.cue = Some(text(c.bytes));
                        }
                    }
                }
            }
        } else if f.field == 2 && f.wire == WIRE_LEN && out.destination_address.is_none() {
            for d in decode_fields(f.bytes) {
                if d.field == 1 && d.wire == WIRE_LEN {
                    out.destination_address = Some(text(d.bytes));
                }
            }
        }
    }
    out
}

pub fn decode_position(payload: &[u8]) -> NavigationPositionUpdate {
    let mut out = NavigationPositionUpdate::default();
    for f in decode_fields(payload) {
        if f.field == 1 && f.wire == WIRE_LEN {
            for s in decode_fields(f.bytes) {
                if s.field == 1 && s.wire == WIRE_LEN {
                    let d = decode_distance(s.bytes);
                    out.step_distance_meters = d.meters;
                    out.step_distance_display = d.display;
                } else if s.field == 2 && s.wire == WIRE_VARINT {
                    out.time_to_step_seconds = Some(decode_varint_value(s.bytes));
                }
            }
        } else if f.field == 2 && f.wire == WIRE_LEN && out.destination_meters.is_none() {
            for dd in decode_fields(f.bytes) {
                if dd.field == 1 && dd.wire == WIRE_LEN {
                    let d = decode_distance(dd.bytes);
                    out.destination_meters = d.meters;
                    out.destination_display = d.display;
                    out.destination_units = d.units;
                } else if dd.field == 2 && dd.wire == WIRE_LEN {
                    out.eta_text = Some(text(dd.bytes));
                } else if dd.field == 3 && dd.wire == WIRE_VARINT {
                    out.time_to_arrival_seconds = Some(decode_varint_value(dd.bytes));
                }
            }
        } else if f.field == 3 && f.wire == WIRE_LEN {
            for r in decode_fields(f.bytes) {
                if r.field == 1 && r.wire == WIRE_LEN {
                    out.current_road_name = Some(text(r.bytes));
                }
            }
        }
    }
    out
}

#[derive(Default)]
struct Distance {
    meters: Option<u32>,
    display: Option<String>,
    units: Option<u32>,
}

fn decode_distance(b: &[u8]) -> Distance {
    let mut out = Distance::default();
    for f in decode_fields(b) {
        if f.field == 1 && f.wire == WIRE_VARINT {
            out.meters = Some(decode_varint_value(f.bytes));
        } else if f.field == 2 && f.wire == WIRE_LEN {
            out.display = Some(text(f.bytes));
        } else if f.field == 3 && f.wire == WIRE_VARINT {
            out.units = Some(decode_varint_value(f.bytes));
        }
    }
    out
}

fn next_turn(v: u32) -> NavigationTurnEvent {
    use NavigationTurnEvent as E;
    match v {
        1 => E::Depart,
        2 => E::NameChange,
        3 => E::SlightTurn,
        4 => E::Turn,
        5 => E::SharpTurn,
        6 => E::UTurn,
        7 => E::OnRamp,
        8 => E::OffRamp,
        9 => E::Fork,
        10 => E::Merge,
        11 => E::RoundaboutEnter,
        12 => E::RoundaboutExit,
        13 => E::RoundaboutEnterAndExit,
        14 => E::Straight,
        16 => E::FerryBoat,
        17 => E::FerryTrain,
        19 => E::Destination,
        _ => E::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{field_len_delim, field_varint};

    fn s(field: u32, v: &str) -> Vec<u8> {
        field_len_delim(field, v.as_bytes())
    }

    #[test]
    fn the_channel_sorts_its_messages() {
        let mut n = NavigationChannel;
        assert_eq!(n.handle_message(nav_msg::START, &[]), Some(NavigationEvent::Start));
        assert_eq!(n.handle_message(nav_msg::STOP, &[]), Some(NavigationEvent::Stop));
        for (raw, state) in [
            (1, NavigationState::Active),
            (2, NavigationState::Inactive),
            (3, NavigationState::Rerouting),
            (7, NavigationState::Unavailable),
        ] {
            assert_eq!(
                n.handle_message(nav_msg::STATUS, &field_varint(1, raw)),
                Some(NavigationEvent::Status(state))
            );
        }
        let turn = [
            s(1, "Main St"),
            field_varint(2, 2),
            field_varint(3, 4),
            field_len_delim(4, &[9]),
            field_varint(5, 2),
            field_varint(6, 90),
        ]
        .concat();
        assert_eq!(
            n.handle_message(nav_msg::NEXT_TURN, &turn),
            Some(NavigationEvent::Turn(NavigationTurnUpdate {
                road: Some("Main St".into()),
                turn_side: Some(NavigationTurnSide::Right),
                event: Some(NavigationTurnEvent::Turn),
                image: Some(vec![9]),
                turn_number: Some(2),
                turn_angle: Some(90),
            }))
        );
        let dist =
            [field_varint(1, 250), field_varint(2, 20), field_varint(3, 250), field_varint(4, 1)]
                .concat();
        assert_eq!(
            n.handle_message(nav_msg::NEXT_TURN_DISTANCE, &dist),
            Some(NavigationEvent::Distance(NavigationDistanceUpdate {
                distance_meters: 250,
                time_to_turn_seconds: 20,
                display_distance_e3: Some(250),
                display_unit: Some(1),
            }))
        );
        assert_eq!(n.handle_message(0x8999, &[]), None);
    }

    #[test]
    fn state_and_position_take_the_first_step_and_destination() {
        let step = [
            field_len_delim(1, &field_varint(1, 8)),
            field_len_delim(2, &s(1, "Ring")),
            field_len_delim(4, &[s(1, "cue1"), s(1, "cue2")].concat()),
        ]
        .concat();
        let second =
            [field_len_delim(1, &field_varint(1, 7)), field_len_delim(2, &s(1, "Other"))].concat();
        let state = [
            field_len_delim(1, &step),
            field_len_delim(1, &second),
            field_len_delim(2, &s(1, "Home")),
            field_len_delim(2, &s(1, "Work")),
        ]
        .concat();
        assert_eq!(
            decode_state(&state),
            NavigationStateUpdate {
                maneuver_type: Some(8),
                road_name: Some("Ring".into()),
                cue: Some("cue1".into()),
                destination_address: Some("Home".into()),
            }
        );
        let distance =
            |m: i64, d: &str, u: i64| [field_varint(1, m), s(2, d), field_varint(3, u)].concat();
        let position = [
            field_len_delim(
                1,
                &[field_len_delim(1, &distance(300, "300 m", 1)), field_varint(2, 25)].concat(),
            ),
            field_len_delim(
                2,
                &[
                    field_len_delim(1, &distance(12000, "12 km", 2)),
                    s(2, "21:58"),
                    field_varint(3, 900),
                ]
                .concat(),
            ),
            field_len_delim(2, &field_len_delim(1, &distance(1, "x", 1))),
            field_len_delim(3, &s(1, "B1")),
        ]
        .concat();
        assert_eq!(
            decode_position(&position),
            NavigationPositionUpdate {
                step_distance_meters: Some(300),
                step_distance_display: Some("300 m".into()),
                time_to_step_seconds: Some(25),
                destination_meters: Some(12000),
                destination_display: Some("12 km".into()),
                destination_units: Some(2),
                eta_text: Some("21:58".into()),
                time_to_arrival_seconds: Some(900),
                current_road_name: Some("B1".into()),
            }
        );
        let mut n = NavigationChannel;
        assert!(matches!(
            n.handle_message(nav_msg::STATE, &state),
            Some(NavigationEvent::State(_))
        ));
        assert!(matches!(
            n.handle_message(nav_msg::CURRENT_POSITION, &position),
            Some(NavigationEvent::Position(_))
        ));
        for v in 0..21 {
            let _ = next_turn(v);
        }
        assert_eq!(next_turn(19), NavigationTurnEvent::Destination);
        assert_eq!(next_turn(15), NavigationTurnEvent::Unknown);
    }
}
