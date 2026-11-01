// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::channels::navigation::{NavigationTurnEvent, NavigationTurnSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ManeuverType {
    NoTurn = 0,
    LeftTurn = 1,
    RightTurn = 2,
    Straight = 3,
    UTurn = 4,
    FollowRoad = 5,
    EnterRoundabout = 6,
    ExitRoundabout = 7,
    RampOff = 8,
    RampOn = 9,
    EndOfNavigation = 10,
    ProceedToRoute = 11,
    Arrived = 12,
    KeepLeft = 13,
    KeepRight = 14,
    EnterFerry = 15,
    ExitFerry = 16,
    ChangeFerry = 17,
    UTurnToRoute = 18,
    RoundaboutUTurn = 19,
    EndOfRoadLeft = 20,
    EndOfRoadRight = 21,
    RampOffLeft = 22,
    RampOffRight = 23,
    ArrivedLeft = 24,
    ArrivedRight = 25,
    UTurnWhenPossible = 26,
    EndOfDirections = 27,
    RoundaboutExit1 = 28,
    RoundaboutExit2 = 29,
    RoundaboutExit3 = 30,
    RoundaboutExit4 = 31,
    RoundaboutExit5 = 32,
    RoundaboutExit6 = 33,
    RoundaboutExit7 = 34,
    RoundaboutExit8 = 35,
    RoundaboutExit9 = 36,
    RoundaboutExit10 = 37,
    RoundaboutExit11 = 38,
    RoundaboutExit12 = 39,
    RoundaboutExit13 = 40,
    RoundaboutExit14 = 41,
    RoundaboutExit15 = 42,
    RoundaboutExit16 = 43,
    RoundaboutExit17 = 44,
    RoundaboutExit18 = 45,
    RoundaboutExit19 = 46,
    SharpLeft = 47,
    SharpRight = 48,
    SlightLeft = 49,
    SlightRight = 50,
    ChangeHighway = 51,
    ChangeHighwayLeft = 52,
    ChangeHighwayRight = 53,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DrivingSide {
    Right = 0,
    Left = 1,
}

pub fn turn_event_to_maneuver_type(
    event: Option<NavigationTurnEvent>,
    side: Option<NavigationTurnSide>,
) -> Option<ManeuverType> {
    use ManeuverType as M;
    use NavigationTurnEvent as E;
    let event = event?;
    let left = side == Some(NavigationTurnSide::Left);
    let right = side == Some(NavigationTurnSide::Right);
    Some(match event {
        E::Unknown => M::NoTurn,
        E::Depart => M::ProceedToRoute,
        E::NameChange => M::FollowRoad,
        E::SlightTurn if right => M::SlightRight,
        E::SlightTurn => M::SlightLeft,
        E::Turn if right => M::RightTurn,
        E::Turn => M::LeftTurn,
        E::SharpTurn if right => M::SharpRight,
        E::SharpTurn => M::SharpLeft,
        E::UTurn => M::UTurn,
        E::OnRamp => M::RampOn,
        E::OffRamp if right => M::RampOffRight,
        E::OffRamp if left => M::RampOffLeft,
        E::OffRamp => M::RampOff,
        E::Fork if right => M::KeepRight,
        E::Fork => M::KeepLeft,
        E::Merge => M::RampOn,
        E::RoundaboutEnter => M::EnterRoundabout,
        E::RoundaboutExit => M::ExitRoundabout,
        E::RoundaboutEnterAndExit => M::EnterRoundabout,
        E::Straight => M::Straight,
        E::FerryBoat | E::FerryTrain => M::EnterFerry,
        E::Destination if right => M::ArrivedRight,
        E::Destination if left => M::ArrivedLeft,
        E::Destination => M::Arrived,
    })
}

pub fn turn_side_to_navi_code(side: Option<NavigationTurnSide>) -> Option<DrivingSide> {
    match side {
        Some(NavigationTurnSide::Left) => Some(DrivingSide::Left),
        Some(NavigationTurnSide::Right) => Some(DrivingSide::Right),
        _ => None,
    }
}

pub fn nav_maneuver_type_to_code(t: Option<u32>) -> Option<ManeuverType> {
    use ManeuverType as M;
    Some(match t? {
        0 => M::NoTurn,
        1 => M::ProceedToRoute,
        2 => M::FollowRoad,
        3 => M::KeepLeft,
        4 => M::KeepRight,
        5 => M::SlightLeft,
        6 => M::SlightRight,
        7 => M::LeftTurn,
        8 => M::RightTurn,
        9 => M::SharpLeft,
        10 => M::SharpRight,
        11 | 12 => M::UTurn,
        13..=20 => M::RampOn,
        21 | 23 => M::RampOffLeft,
        22 | 24 => M::RampOffRight,
        25 => M::KeepLeft,
        26 => M::KeepRight,
        27..=29 => M::RampOn,
        30 => M::EnterRoundabout,
        31 => M::ExitRoundabout,
        32..=35 => M::EnterRoundabout,
        36 => M::Straight,
        37 | 38 => M::EnterFerry,
        39 | 40 => M::Arrived,
        41 => M::ArrivedLeft,
        42 => M::ArrivedRight,
        _ => return None,
    })
}

pub fn nav_maneuver_type_to_side(t: Option<u32>) -> Option<DrivingSide> {
    match t? {
        3 | 5 | 7 | 9 | 11 | 25 | 41 => Some(DrivingSide::Left),
        4 | 6 | 8 | 10 | 12 | 26 | 42 => Some(DrivingSide::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_pick_the_maneuver() {
        use NavigationTurnEvent as E;
        let l = Some(NavigationTurnSide::Left);
        let r = Some(NavigationTurnSide::Right);
        let u = Some(NavigationTurnSide::Unspecified);
        assert_eq!(turn_event_to_maneuver_type(None, r), None);
        assert_eq!(
            turn_event_to_maneuver_type(Some(E::Turn), r),
            Some(ManeuverType::RightTurn)
        );
        assert_eq!(
            turn_event_to_maneuver_type(Some(E::Turn), u),
            Some(ManeuverType::LeftTurn)
        );
        assert_eq!(
            turn_event_to_maneuver_type(Some(E::OffRamp), l),
            Some(ManeuverType::RampOffLeft)
        );
        assert_eq!(
            turn_event_to_maneuver_type(Some(E::OffRamp), None),
            Some(ManeuverType::RampOff)
        );
        assert_eq!(
            turn_event_to_maneuver_type(Some(E::Destination), l),
            Some(ManeuverType::ArrivedLeft)
        );
        assert_eq!(turn_side_to_navi_code(u), None);
        assert_eq!(turn_side_to_navi_code(l), Some(DrivingSide::Left));
        assert_eq!(
            nav_maneuver_type_to_code(Some(18)),
            Some(ManeuverType::RampOn)
        );
        assert_eq!(nav_maneuver_type_to_code(Some(43)), None);
        assert_eq!(nav_maneuver_type_to_code(None), None);
        assert_eq!(
            nav_maneuver_type_to_side(Some(12)),
            Some(DrivingSide::Right)
        );
        assert_eq!(nav_maneuver_type_to_side(Some(13)), None);
    }
}
