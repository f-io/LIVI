// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod calibration;
pub mod color;
pub mod custom;
pub mod node;
pub mod restart;
pub mod schemas;
pub mod sources;
pub mod value;
pub mod view;

use node::{Label, Node};

pub const STATE_READ: [&str; 10] = [
    "config",
    "system",
    "telemetry",
    "mfi",
    "dab",
    "update",
    "sessions",
    "devices",
    "cluster",
    "cameras",
];

pub fn schema() -> Node {
    Node::route(
        "settings",
        Label::new("settings.settingsTitle", "Settings"),
        vec![
            schemas::devices::schema(),
            schemas::general::schema(),
            schemas::car::schema(),
            schemas::audio::schema(),
            schemas::video::schema(),
            schemas::appearance::schema(),
            schemas::system::schema(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::node::{Kind, Label, Node};
    use super::*;
    use crate::i18n::Strings;
    use crate::icons::settings_icon;

    fn read_keys(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                read_keys(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for part in text.split("state[\"").skip(1) {
                    if let Some((key, _)) = part.split_once('"') {
                        out.push(key.to_string());
                    }
                }
            }
        }
    }

    #[test]
    fn every_state_key_a_page_reads_redraws_the_pages() {
        let mut keys = Vec::new();
        read_keys(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/settings"),
            &mut keys,
        );
        keys.sort();
        keys.dedup();
        assert!(keys.len() > 5);
        let missing: Vec<_> = keys
            .iter()
            .filter(|k| !STATE_READ.contains(&k.as_str()))
            .collect();
        assert!(missing.is_empty(), "add to STATE_READ: {missing:?}");
    }

    fn walk<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
        out.push(n);
        n.children().iter().for_each(|c| walk(c, out));
    }

    fn labels(n: &Node) -> Vec<Label> {
        let mut out = vec![n.label];
        out.extend(n.page_title);
        if let Kind::Select { options, .. } = &n.kind {
            out.extend(options.iter().map(|c| c.label));
        }
        out
    }

    #[test]
    fn every_icon_key_has_an_icon() {
        let root = schema();
        let mut all = Vec::new();
        walk(&root, &mut all);
        let missing: Vec<_> = all
            .iter()
            .filter_map(|n| n.icon)
            .filter(|k| settings_icon(k).is_none())
            .collect();
        assert!(missing.is_empty(), "no icon for {missing:?}");
    }

    #[test]
    fn every_label_key_is_translated() {
        let en = Strings::new("en");
        let root = schema();
        let mut all = Vec::new();
        walk(&root, &mut all);
        let missing: Vec<_> = all
            .iter()
            .flat_map(|n| labels(n))
            .filter_map(|l| l.key)
            .filter(|k| en.get(k).is_none())
            .collect();
        assert!(missing.is_empty(), "not in en.json: {missing:?}");
    }

    #[test]
    fn every_route_and_custom_page_opens() {
        let root = schema();
        let mut all = Vec::new();
        walk(&root, &mut all);
        assert!(all.len() > 200);
        assert!(
            all.iter()
                .filter(|n| matches!(n.kind, Kind::Custom(_)))
                .count()
                == 18
        );
    }
}
