// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::message::Action;
use livi_core_proto::state::Dab;
use serde_json::Value;

use super::{FAVORITE_TINT, info};
use crate::i18n::Strings;
use crate::icons::settings_icon;
use crate::media::{badge, initials};
use crate::settings::view::{Badge, Row, RowKind, Tool};

pub fn dab(state: &Value) -> Dab {
    serde_json::from_value(state["dab"].clone()).unwrap_or_default()
}

pub fn receiver(d: &Dab, t: &Strings) -> Vec<Row> {
    let mut out = vec![info(
        t.t("settings.dabReceiver", "Receiver"),
        Some("dabReceiver"),
        d.receiver
            .clone()
            .unwrap_or_else(|| t.t("settings.dabNoReceiver", "No stick plugged in")),
    )];
    match &d.scan {
        Some(scan) => {
            out.push(info(
                t.with(
                    "settings.dabScanning",
                    "Scanning {{channel}}",
                    &[("channel", &scan.channel)],
                ),
                Some("dabStations"),
                t.with(
                    "settings.dabFound",
                    "{{count}} found",
                    &[("count", &scan.found.to_string())],
                ),
            ));
            out.push(Row {
                kind: RowKind::Progress,
                level: if scan.total > 0 {
                    scan.done as f32 / scan.total as f32
                } else {
                    0.0
                },
                enabled: true,
                ..Row::default()
            });
            out.push(Row {
                kind: RowKind::Button,
                button: t.t("settings.dabStopScan", "Stop scan"),
                outlined: true,
                action: Some(Action::DabStopScan),
                enabled: true,
                ..Row::default()
            });
        }
        None => out.push(Row {
            kind: RowKind::Button,
            label: t.with(
                "settings.dabStations",
                "{{count}} stations",
                &[("count", &d.stations.len().to_string())],
            ),
            icon: settings_icon("dabStations"),
            button: t.t("settings.dabScan", "Scan"),
            action: Some(Action::DabScan),
            enabled: d.receiver.is_some(),
            ..Row::default()
        }),
    }
    if let Some(e) = &d.error {
        out.push(info(t.t("settings.dabError", "Error"), None, e.clone()));
    }
    out
}

pub fn stations(d: &Dab, t: &Strings) -> Vec<Row> {
    if d.stations.is_empty() {
        return vec![info(
            t.t(
                "media.noStations",
                "No stations yet, scan under Settings > General > DAB+",
            ),
            None,
            String::new(),
        )];
    }
    let last = d.stations.len() - 1;
    d.stations
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let id = st.service_id;
            Row {
                kind: RowKind::Item,
                label: st.label.clone(),
                enabled: true,
                dimmed: st.hidden,
                badge: Some(Badge {
                    initials: initials(&st.label),
                    color: badge(&st.label),
                    logo: st.logo.clone(),
                }),
                tools: vec![
                    Tool {
                        icon: if st.favorite {
                            "StarRounded"
                        } else {
                            "StarBorderRounded"
                        },
                        label: t.t("media.favorite", "Favorite"),
                        on: st.favorite,
                        tint: st.favorite.then_some(FAVORITE_TINT),
                        enabled: true,
                        switch: false,
                        action: Action::DabFavorite {
                            service_id: id,
                            favorite: !st.favorite,
                        },
                    },
                    Tool {
                        icon: if st.hidden {
                            "VisibilityOffRounded"
                        } else {
                            "VisibilityRounded"
                        },
                        label: if st.hidden {
                            t.t("media.showStation", "Show station")
                        } else {
                            t.t("media.hideStation", "Hide station")
                        },
                        on: st.hidden,
                        tint: None,
                        enabled: true,
                        switch: false,
                        action: Action::DabHide {
                            service_id: id,
                            hidden: !st.hidden,
                        },
                    },
                    Tool {
                        icon: "ExpandLess",
                        label: t.t("settings.moveUp", "Move up"),
                        on: false,
                        tint: None,
                        enabled: i > 0,
                        switch: false,
                        action: Action::DabMove {
                            service_id: id,
                            by: -1,
                        },
                    },
                    Tool {
                        icon: "ExpandMore",
                        label: t.t("settings.moveDown", "Move down"),
                        on: false,
                        tint: None,
                        enabled: i < last,
                        switch: false,
                        action: Action::DabMove {
                            service_id: id,
                            by: 1,
                        },
                    },
                ],
                ..Row::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings::custom::rows;
    use crate::settings::node::CustomPage;
    use crate::settings::view::Facts;

    fn strings() -> Strings {
        Strings::new("en")
    }

    #[test]
    fn receiver_offers_a_scan_only_with_a_stick() {
        let none = rows(
            CustomPage::DabReceiver,
            &json!({}),
            &strings(),
            &Facts::default(),
        )
        .unwrap();
        assert_eq!(none[0].value, "No stick plugged in");
        assert_eq!(none[1].kind, RowKind::Button);
        assert!(!none[1].enabled);
        let stick = json!({ "dab": { "receiver": "RTL2838", "stations": [], "active": false,
            "playing": false, "signal": { "locked": false, "quality": 0 }, "gainsDb": [] } });
        let some = rows(
            CustomPage::DabReceiver,
            &stick,
            &strings(),
            &Facts::default(),
        )
        .unwrap();
        assert!(some[1].enabled);
        assert_eq!(some[1].action, Some(Action::DabScan));
    }

    #[test]
    fn a_running_scan_shows_progress_and_stop() {
        let state = json!({ "dab": { "receiver": "RTL2838", "stations": [], "active": false,
            "playing": false, "signal": { "locked": false, "quality": 0 }, "gainsDb": [],
            "scan": { "channel": "5C", "done": 10, "total": 40, "found": 3 } } });
        let r = rows(
            CustomPage::DabReceiver,
            &state,
            &strings(),
            &Facts::default(),
        )
        .unwrap();
        assert_eq!(r[1].label, "Scanning 5C");
        assert_eq!(r[1].value, "3 found");
        assert_eq!(r[2].kind, RowKind::Progress);
        assert!((r[2].level - 0.25).abs() < 1e-6);
        assert_eq!(r[3].action, Some(Action::DabStopScan));
    }

    #[test]
    fn stations_carry_their_tools() {
        let state = json!({ "dab": { "receiver": null, "active": false, "playing": false,
            "signal": { "locked": false, "quality": 0 }, "gainsDb": [], "stations": [
            { "serviceId": 1, "label": "Radio Eins", "shortLabel": "", "ensemble": "", "channel": "7D", "frequencyHz": 1, "favorite": true },
            { "serviceId": 2, "label": "Fritz", "shortLabel": "", "ensemble": "", "channel": "7D", "frequencyHz": 1, "hidden": true }
        ] } });
        let r = rows(
            CustomPage::DabStations,
            &state,
            &strings(),
            &Facts::default(),
        )
        .unwrap();
        assert_eq!(r.len(), 2);
        assert!(r[0].tools[0].on && !r[0].tools[2].enabled);
        assert!(r[1].dimmed && !r[1].tools[3].enabled);
        assert_eq!(
            r[1].tools[2].action,
            Action::DabMove {
                service_id: 2,
                by: -1
            }
        );
    }
}
