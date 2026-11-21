// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

use crate::settings::node::{Kind, Node};
use crate::settings::sources::holds;
use crate::settings::value::get;

pub const KEY: &str = "hiddenSettings";

pub fn entries(config: &Value) -> Vec<&str> {
    config[KEY]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn ends_with(chain: &[&str], entry: &str) -> bool {
    let wanted: Vec<&str> = entry.split('.').collect();
    chain.len() >= wanted.len() && chain[chain.len() - wanted.len()..] == wanted[..]
}

fn names_key(node: &Node, entry: &str) -> bool {
    !matches!(node.kind, Kind::Custom(_) | Kind::Route { .. })
        && !node.path.is_empty()
        && (node.path == entry
            || node
                .path
                .strip_prefix(entry)
                .is_some_and(|rest| rest.starts_with('.')))
}

pub fn shows(node: &Node, chain: &[&'static str], state: &Value) -> bool {
    if node.hidden.is_some_and(|c| holds(c, &state["system"])) {
        return false;
    }
    let mut own = chain.to_vec();
    if let Some(route) = node.route_key() {
        own.push(route);
    }
    let hidden = entries(&state["config"]);
    if hidden
        .iter()
        .any(|e| ends_with(&own, e) || names_key(node, e))
    {
        return false;
    }
    match &node.kind {
        Kind::Route { children, .. } if !children.is_empty() => {
            children.iter().any(|c| shows(c, &own, state))
        }
        _ => true,
    }
}

fn routes(node: &Node, chain: &mut Vec<&'static str>, out: &mut Vec<Vec<&'static str>>) {
    for child in node.children() {
        if let Some(route) = child.route_key() {
            chain.push(route);
            out.push(chain.clone());
            routes(child, chain, out);
            chain.pop();
        }
    }
}

pub fn known(entry: &str, root: &Node, defaults: &Value) -> bool {
    if entry.is_empty() {
        return false;
    }
    if !get(defaults, entry).is_null() {
        return true;
    }
    let mut all = Vec::new();
    routes(root, &mut Vec::new(), &mut all);
    all.iter().any(|chain| ends_with(chain, entry))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings::schema;

    fn visible(path: &[&str], hidden: &[&str]) -> bool {
        let root = schema();
        let state = json!({ "config": { KEY: hidden }, "system": { "displaysSettable": true } });
        let mut node = &root;
        let mut chain = Vec::new();
        for key in path {
            assert!(shows(node, &chain, &state) || node.route_key() == Some("settings"));
            if let Some(route) = node.route_key().filter(|r| *r != "settings") {
                chain.push(route);
            }
            node = node
                .children()
                .iter()
                .find(|c| c.route_key() == Some(*key) || c.path == *key)
                .unwrap_or_else(|| panic!("no {key}"));
        }
        shows(node, &chain, &state)
    }

    #[test]
    fn a_route_hides_wherever_it_ends_a_chain() {
        let main = ["general", "screenSettings", "mainScreen", "viewArea"];
        let video = ["video", "mainVideo", "viewArea"];
        assert!(!visible(&main, &["viewArea"]));
        assert!(!visible(&video, &["viewArea"]));
        assert!(!visible(&main, &["mainScreen.viewArea"]));
        assert!(visible(&video, &["mainScreen.viewArea"]));
    }

    #[test]
    fn a_key_hides_itself_and_what_is_below_it() {
        let cluster = ["general", "tabSettings", "dashboard", "dashboard.cluster"];
        let main = ["general", "tabSettings", "dashboard", "dashboard.main"];
        assert!(!visible(&cluster, &["dashboard.cluster"]));
        assert!(visible(&main, &["dashboard.cluster"]));
        assert!(!visible(
            &[
                "general",
                "screenSettings",
                "mainScreen",
                "displayMode.main"
            ],
            &["displayMode"]
        ));
        assert!(visible(
            &[
                "general",
                "screenSettings",
                "mainScreen",
                "displayMode.main"
            ],
            &["displayModeX"]
        ));
    }

    #[test]
    fn a_route_whose_children_all_hide_hides_too() {
        let gps = ["general", "gps"];
        assert!(visible(&gps, &["gpsEnabled", "gpsDevice", "gpsBaudRate"]));
        assert!(!visible(
            &gps,
            &["gpsEnabled", "gpsDevice", "gpsBaudRate", "info", "hwinfo"]
        ));
    }

    #[test]
    fn known_entries_are_routes_or_keys() {
        let root = schema();
        let defaults = serde_json::to_value(livi_core_proto::config::defaults()).unwrap();
        for entry in [
            "viewArea",
            "mainScreen.viewArea",
            "gps",
            "dashboard.cluster",
            "displayMode",
            "audioOutputDevice",
        ] {
            assert!(known(entry, &root, &defaults), "{entry}");
        }
        for entry in ["", "nope", "mainVideo.mainScreen", "dashboard.nope"] {
            assert!(!known(entry, &root, &defaults), "{entry}");
        }
    }
}
