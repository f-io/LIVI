// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_i18n::Strings;

#[derive(Clone, Copy)]
pub struct Language(&'static Strings);

impl Language {
    pub fn of(setting: &str) -> Self {
        Self(livi_i18n::cached(setting))
    }
}

const ROUNDABOUT_EXITS: std::ops::RangeInclusive<u32> = 28..=46;

const MANEUVERS: [(u32, &str, &str); 35] = [
    (0, "noTurn", "No turn"),
    (1, "turnLeft", "Turn left"),
    (2, "turnRight", "Turn right"),
    (3, "straight", "Go straight"),
    (4, "uTurn", "Make a U-turn"),
    (5, "continue", "Continue on the current road"),
    (6, "enterRoundabout", "Enter roundabout"),
    (7, "exitRoundabout", "Exit roundabout"),
    (8, "exitHighway", "Exit highway"),
    (9, "mergeHighway", "Merge onto highway"),
    (10, "endOfNavigation", "End of navigation"),
    (11, "proceedToRoute", "Proceed to the route"),
    (12, "arrived", "Arrived"),
    (13, "keepLeft", "Keep left"),
    (14, "keepRight", "Keep right"),
    (15, "enterFerry", "Enter ferry"),
    (16, "exitFerry", "Exit ferry"),
    (17, "changeFerry", "Change ferry"),
    (18, "uTurnToRoute", "Make a U-turn to rejoin the route"),
    (19, "roundaboutUTurn", "Use the roundabout to make a U-turn"),
    (20, "endOfRoadLeft", "At the end of the road, turn left"),
    (21, "endOfRoadRight", "At the end of the road, turn right"),
    (22, "exitHighwayLeft", "Exit highway on the left"),
    (23, "exitHighwayRight", "Exit highway on the right"),
    (24, "arrivedLeft", "Arrived (left)"),
    (25, "arrivedRight", "Arrived (right)"),
    (26, "uTurnWhenPossible", "Make a U-turn when possible"),
    (27, "endOfDirections", "End of directions"),
    (47, "sharpLeft", "Sharp left"),
    (48, "sharpRight", "Sharp right"),
    (49, "slightLeft", "Slight left"),
    (50, "slightRight", "Slight right"),
    (51, "changeHighway", "Change highway"),
    (52, "changeHighwayLeft", "Change highway (left)"),
    (53, "changeHighwayRight", "Change highway (right)"),
];

pub fn maneuver(code: u32, language: Language) -> Option<String> {
    if ROUNDABOUT_EXITS.contains(&code) {
        let exit = (code - ROUNDABOUT_EXITS.start() + 1).to_string();
        return Some(language.0.with(
            "navigation.roundaboutExit",
            "Roundabout exit {{n}}",
            &[("n", &exit)],
        ));
    }
    MANEUVERS
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, key, text)| language.0.t(&format!("navigation.{key}"), text))
}

pub fn unknown_maneuver(language: Language) -> &'static str {
    language.0.get("navigation.unknown").unwrap_or("Unknown")
}

pub fn no_route(language: Language) -> &'static str {
    language.0.get("navigation.noRoute").unwrap_or("No Route")
}

pub fn distance(meters: f64) -> Option<String> {
    if !meters.is_finite() || meters < 0.0 {
        return None;
    }
    if meters < 1000.0 {
        return Some(format!("{} m", meters.round()));
    }
    let km = meters / 1000.0;
    if km < 10.0 {
        Some(format!("{} km", one_decimal(km)))
    } else {
        Some(format!("{} km", km.round()))
    }
}

fn one_decimal(v: f64) -> String {
    let exact = format!("{v:.30}");
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, "0"));
    let mut tenths: u64 = whole.parse::<u64>().unwrap_or(0) * 10;
    let mut digits = fraction.bytes();
    tenths += u64::from(digits.next().unwrap_or(b'0') - b'0');
    if digits.next().unwrap_or(b'0') >= b'5' {
        tenths += 1;
    }
    format!("{}.{}", tenths / 10, tenths % 10)
}

pub fn time_left(seconds: f64) -> Option<String> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let total = seconds.floor() as u64;
    let (hours, minutes) = (total / 3600, total % 3600 / 60);
    if hours > 0 {
        Some(format!("{hours}:{minutes:02} h"))
    } else {
        Some(format!("{minutes} min"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn vectors() -> Value {
        serde_json::from_str(include_str!("nav_text_vectors.json")).unwrap()
    }

    #[test]
    fn every_maneuver_reads_as_the_old_dictionary_had_it() {
        let v = vectors();
        let language = Language::of("en");
        for (code, text) in v["en"]["maneuvers"].as_object().unwrap() {
            let code: i64 = code.parse().unwrap();
            let ours = u32::try_from(code)
                .ok()
                .and_then(|c| maneuver(c, language))
                .unwrap_or_else(|| unknown_maneuver(language).to_string());
            assert_eq!(ours, text.as_str().unwrap(), "{code}");
        }
        assert_eq!(unknown_maneuver(language), v["en"]["none"]);
    }

    #[test]
    fn distances_and_times_read_as_before() {
        let v = vectors();
        for (meters, text) in v["meters"].as_object().unwrap() {
            assert_eq!(
                distance(meters.parse().unwrap()).as_deref(),
                text.as_str(),
                "{meters} m"
            );
        }
        for (seconds, text) in v["seconds"].as_object().unwrap() {
            assert_eq!(
                time_left(seconds.parse().unwrap()).as_deref(),
                text.as_str(),
                "{seconds} s"
            );
        }
        assert_eq!(distance(1250.0).as_deref(), Some("1.3 km"));
        assert_eq!(distance(f64::NAN), None);
    }

    #[test]
    fn unknown_languages_read_english() {
        assert_eq!(no_route(Language::of("xx")), "No Route");
    }
}
