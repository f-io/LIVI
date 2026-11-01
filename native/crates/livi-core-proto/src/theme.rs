// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub id: String,
    pub displays: Vec<ThemeDisplay>,
    pub palettes: Vec<Palette>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDisplay {
    pub id: String,
    pub kind: DisplayKind,
    pub layouts: Vec<ThemeLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayKind {
    Cluster,
    Center,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeLayout {
    pub id: String,
    pub name: String,
    pub palettes: Vec<String>,
    pub zones: Vec<Zone>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub id: String,
    pub component: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub id: String,
    pub accent_dark: String,
    pub accent_light: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearancePreference {
    #[default]
    Automatic,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub theme: Theme,
    pub layout: String,
    pub palette: String,
    pub palettes: BTreeMap<String, String>,
    pub shown: Vec<String>,
    pub arranged: Vec<ArrangedLayout>,
    pub appearance: AppearancePreference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangedLayout {
    pub id: String,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSlot {
    pub on: bool,
    pub pos: u32,
}

impl Theme {
    pub fn cluster(&self) -> Option<&ThemeDisplay> {
        self.displays
            .iter()
            .find(|d| d.kind == DisplayKind::Cluster)
    }

    pub fn cluster_layout(&self, id: &str) -> Option<&ThemeLayout> {
        self.cluster()?.layouts.iter().find(|l| l.id == id)
    }
}

impl Cluster {
    pub fn new(
        theme: Theme,
        layout: &str,
        palettes: &BTreeMap<String, String>,
        slots: &BTreeMap<String, LayoutSlot>,
    ) -> Self {
        let mut c = Self {
            theme,
            ..Self::default()
        };
        c.arrange(slots);
        c.palettes = palettes
            .iter()
            .filter(|(l, _)| c.theme.cluster_layout(l).is_some())
            .map(|(l, p)| (l.clone(), p.clone()))
            .collect();
        let first = c
            .theme
            .cluster()
            .and_then(|d| d.layouts.first())
            .map(|l| l.id.clone());
        let start = if c.theme.cluster_layout(layout).is_some() {
            Some(layout.to_string())
        } else {
            first
        };
        if let Some(start) = start {
            c.select(&start, None);
        }
        c
    }

    pub fn step(&mut self, by: i32) -> bool {
        let n = self.shown.len() as i32;
        if n == 0 {
            return false;
        }
        let next = match self.shown.iter().position(|l| *l == self.layout) {
            Some(at) => self.shown[(at as i32 + by).rem_euclid(n) as usize].clone(),
            None => self.shown[0].clone(),
        };
        self.select(&next, None)
    }

    pub fn arrange(&mut self, slots: &BTreeMap<String, LayoutSlot>) -> bool {
        let all: Vec<&ThemeLayout> = self
            .theme
            .cluster()
            .map(|d| d.layouts.iter().collect())
            .unwrap_or_default();
        let mut ordered: Vec<(u32, usize, &ThemeLayout)> = all
            .iter()
            .enumerate()
            .map(|(i, l)| (slots.get(&l.id).map_or(u32::MAX, |s| s.pos), i, *l))
            .collect();
        ordered.sort_by_key(|(pos, i, _)| (*pos, *i));
        let arranged: Vec<ArrangedLayout> = ordered
            .iter()
            .map(|(_, _, l)| ArrangedLayout {
                id: l.id.clone(),
                on: slots.get(&l.id).is_none_or(|s| s.on),
            })
            .collect();
        let mut shown: Vec<String> = arranged
            .iter()
            .filter(|l| l.on)
            .map(|l| l.id.clone())
            .collect();
        if shown.is_empty() {
            shown = arranged.iter().map(|l| l.id.clone()).collect();
        }
        let changed = shown != self.shown || arranged != self.arranged;
        self.shown = shown;
        self.arranged = arranged;
        changed
    }

    pub fn moved(&self, layout: &str, by: i32) -> Option<BTreeMap<String, LayoutSlot>> {
        let at = self.arranged.iter().position(|l| l.id == layout)?;
        let to = at
            .checked_add_signed(by as isize)
            .filter(|to| *to < self.arranged.len())?;
        let mut list = self.arranged.clone();
        let item = list.remove(at);
        list.insert(to, item);
        Some(slots_of(&list))
    }

    pub fn switched(&self, layout: &str, on: bool) -> Option<BTreeMap<String, LayoutSlot>> {
        let mut list = self.arranged.clone();
        list.iter_mut().find(|l| l.id == layout)?.on = on;
        Some(slots_of(&list))
    }

    pub fn select(&mut self, layout: &str, palette: Option<&str>) -> bool {
        let Some(found) = self.theme.cluster_layout(layout) else {
            return false;
        };
        let offered = |p: &&str| found.palettes.iter().any(|q| q == p);
        let palette = palette
            .filter(offered)
            .or(self.palettes.get(layout).map(String::as_str))
            .or(found.palettes.first().map(String::as_str))
            .unwrap_or_default()
            .to_string();
        self.show(layout, palette)
    }

    pub fn follow(&mut self, layout: &str, palette: Option<&str>) -> bool {
        if self.theme.cluster_layout(layout).is_none() {
            return false;
        }
        match palette {
            Some(p) => self.show(layout, p.to_string()),
            None if self.layout == layout => false,
            None => self.select(layout, None),
        }
    }

    fn show(&mut self, layout: &str, palette: String) -> bool {
        self.palettes.insert(layout.to_string(), palette.clone());
        if self.layout == layout && self.palette == palette {
            return false;
        }
        self.layout = layout.to_string();
        self.palette = palette;
        true
    }
}

fn slots_of(list: &[ArrangedLayout]) -> BTreeMap<String, LayoutSlot> {
    list.iter()
        .zip(1..)
        .map(|(l, pos)| (l.id.clone(), LayoutSlot { on: l.on, pos }))
        .collect()
}

const LIVI_CLUSTER: &str = "Cluster_Display";
const LIVI_CENTER: &str = "Center_Display";
const MAP_ZONE: &str = "map";

pub fn livi_theme() -> Theme {
    let layout = |id: &str, name: &str, map: &str| ThemeLayout {
        id: id.into(),
        name: name.into(),
        palettes: vec!["default".into()],
        zones: vec![Zone {
            id: MAP_ZONE.into(),
            component: map.into(),
        }],
    };
    Theme {
        id: "livi".into(),
        displays: vec![
            ThemeDisplay {
                id: LIVI_CLUSTER.into(),
                kind: DisplayKind::Cluster,
                layouts: vec![
                    layout("livi.navMini", "Gauges with turn-by-turn", "maneuver"),
                    layout(
                        "livi.navFull",
                        "Gauges with turn-by-turn (extended)",
                        "maneuver",
                    ),
                    layout("livi.clusterFull", "Gauges with map", "map"),
                    layout("livi.clusterOnly", "Map", "map"),
                ],
            },
            ThemeDisplay {
                id: LIVI_CENTER.into(),
                kind: DisplayKind::Center,
                layouts: Vec::new(),
            },
        ],
        palettes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULAR: &str = "0D38B526-1271-575D-8A92-393A86B718F9";
    const DIGIRING: &str = "8BC9ABB1-CB20-595E-92BF-D8F099BF3515";
    const FIRST: &str = "6C45CC47-EC01-527A-B40D-1316AA9C936F";
    const MAPS: &str = "DB7C6944-5DFF-58DF-94E9-91006781FA93";

    fn maker_theme() -> Theme {
        let layout = |id: &str, palettes: &[&str]| ThemeLayout {
            id: id.into(),
            name: id.into(),
            palettes: palettes.iter().map(|p| p.to_string()).collect(),
            zones: Vec::new(),
        };
        let colours = [
            "custom",
            "green",
            "blue",
            "red",
            "rose",
            "starlight",
            "vibrant",
        ];
        Theme {
            id: "maker".into(),
            displays: vec![ThemeDisplay {
                id: LIVI_CLUSTER.into(),
                kind: DisplayKind::Cluster,
                layouts: vec![
                    layout(FIRST, &["custom"]),
                    layout(DIGIRING, &["green", "blue", "red"]),
                    layout(MODULAR, &["blue", "green", "rose", "starlight"]),
                    layout(MAPS, &["vibrant"]),
                ],
            }],
            palettes: colours
                .iter()
                .map(|p| Palette {
                    id: p.to_string(),
                    accent_dark: "#fff".into(),
                    accent_light: "#000".into(),
                })
                .collect(),
        }
    }

    fn fresh(theme: Theme) -> Cluster {
        Cluster::new(theme, "", &BTreeMap::new(), &BTreeMap::new())
    }

    #[test]
    fn the_cluster_starts_on_the_first_layout_and_wraps_both_ways() {
        let mut c = fresh(livi_theme());
        assert_eq!(
            (c.layout.as_str(), c.palette.as_str()),
            ("livi.navMini", "default")
        );
        assert!(c.step(-1));
        assert_eq!(c.layout, "livi.clusterOnly");
        assert!(c.step(1));
        assert_eq!(c.layout, "livi.navMini");
    }

    #[test]
    fn select_takes_an_offered_palette_and_ignores_unknown_layouts() {
        let mut c = fresh(maker_theme());
        assert!(c.select(MODULAR, Some("rose")));
        assert_eq!(c.palette, "rose");
        assert!(!c.select(MODULAR, Some("rose")), "already up");
        assert!(!c.select("nope", None));
        assert!(c.select("DB7C6944-5DFF-58DF-94E9-91006781FA93", Some("rose")));
        assert_eq!(
            c.palette, "vibrant",
            "a palette the layout does not offer falls back"
        );
    }

    #[test]
    fn a_layout_comes_back_with_the_palette_picked_for_it() {
        let mut c = fresh(maker_theme());
        assert!(c.follow(DIGIRING, Some("blue")));
        assert!(c.select(MODULAR, None));
        assert_eq!(c.palette, "blue", "Modular's own default");
        assert!(c.select(DIGIRING, None));
        assert_eq!(c.palette, "blue");
        assert!(c.follow(DIGIRING, Some("green")));
        assert_eq!(c.palettes.get(DIGIRING).map(String::as_str), Some("green"));
    }

    #[test]
    fn the_layout_and_palettes_kept_from_before_come_back() {
        let kept = BTreeMap::from([
            (DIGIRING.to_string(), "red".to_string()),
            ("gone".to_string(), "blue".to_string()),
        ]);
        let c = Cluster::new(maker_theme(), DIGIRING, &kept, &BTreeMap::new());
        assert_eq!((c.layout.as_str(), c.palette.as_str()), (DIGIRING, "red"));
        assert!(
            !c.palettes.contains_key("gone"),
            "layouts the theme lost are dropped"
        );
        let c = Cluster::new(maker_theme(), "gone", &kept, &BTreeMap::new());
        assert_eq!(c.layout, "6C45CC47-EC01-527A-B40D-1316AA9C936F");
    }

    #[test]
    fn the_phone_may_pick_palettes_livi_does_not_list() {
        let mut c = fresh(maker_theme());
        assert!(c.follow(MODULAR, None));
        assert_eq!(c.palette, "blue");
        assert!(c.follow(MODULAR, Some("racingGreen")));
        assert_eq!(c.palette, "racingGreen");
        assert!(
            !c.follow(MODULAR, None),
            "a layout start keeps the palette it has"
        );
        assert!(!c.follow("nope", Some("x")));
    }

    #[test]
    fn every_offered_palette_has_its_colour() {
        let theme = maker_theme();
        for layout in &theme.cluster().unwrap().layouts {
            for p in &layout.palettes {
                assert!(
                    theme.palettes.iter().any(|q| &q.id == p),
                    "{} {p}",
                    layout.name
                );
            }
        }
    }

    #[test]
    fn the_user_orders_and_switches_off_layouts() {
        let slot = |on, pos| LayoutSlot { on, pos };
        let slots = BTreeMap::from([
            ("livi.clusterOnly".to_string(), slot(true, 1)),
            ("livi.navFull".to_string(), slot(false, 2)),
            ("livi.navMini".to_string(), slot(true, 3)),
        ]);
        let mut c = Cluster::new(livi_theme(), "", &BTreeMap::new(), &slots);
        assert_eq!(
            c.shown,
            ["livi.clusterOnly", "livi.navMini", "livi.clusterFull"]
        );
        assert_eq!(
            c.layout, "livi.navMini",
            "the first theme layout until one is picked"
        );
        assert!(c.step(1));
        assert_eq!(
            c.layout, "livi.clusterFull",
            "the switched off layout is skipped"
        );
        assert!(c.step(1));
        assert_eq!(c.layout, "livi.clusterOnly", "round again");
        assert!(!c.arrange(&slots), "same arrangement");

        let all_off: BTreeMap<_, _> = c
            .shown
            .iter()
            .chain(["livi.navFull".to_string()].iter())
            .map(|l| (l.clone(), slot(false, 0)))
            .collect();
        assert!(c.arrange(&all_off));
        assert_eq!(c.shown.len(), 4, "never empty");

        c.arrange(&BTreeMap::from([(
            "livi.navMini".to_string(),
            slot(false, 1),
        )]));
        assert!(c.select("livi.navMini", None));
        assert!(c.step(1));
        assert_eq!(
            c.layout, "livi.navFull",
            "from a hidden layout to the first shown"
        );
    }

    #[test]
    fn the_user_moves_and_switches_layouts() {
        let mut c = Cluster::new(livi_theme(), "", &BTreeMap::new(), &BTreeMap::new());
        let ids = |c: &Cluster| c.arranged.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&c),
            [
                "livi.navMini",
                "livi.navFull",
                "livi.clusterFull",
                "livi.clusterOnly"
            ]
        );
        assert!(
            c.arranged.iter().all(|l| l.on),
            "a layout without a slot is on"
        );

        let slots = c.moved("livi.clusterOnly", -1).unwrap();
        assert_eq!(slots["livi.clusterOnly"], LayoutSlot { on: true, pos: 3 });
        assert_eq!(slots["livi.clusterFull"], LayoutSlot { on: true, pos: 4 });
        assert!(c.arrange(&slots));
        assert_eq!(ids(&c)[2..], ["livi.clusterOnly", "livi.clusterFull"]);
        assert_eq!(c.moved("livi.navMini", -1), None, "the first stays first");
        assert_eq!(c.moved("livi.clusterFull", 1), None, "the last stays last");
        assert_eq!(c.moved("nope", 1), None);

        let slots = c.switched("livi.navFull", false).unwrap();
        assert_eq!(slots["livi.navFull"], LayoutSlot { on: false, pos: 2 });
        c.arrange(&slots);
        assert_eq!(
            c.shown,
            ["livi.navMini", "livi.clusterOnly", "livi.clusterFull"]
        );
        assert!(
            !c.arranged[1].on,
            "the arrangement keeps the switched off layout"
        );
        assert_eq!(c.switched("nope", true), None);
    }

    #[test]
    fn a_theme_without_cluster_layouts_does_not_move() {
        let mut c = fresh(Theme::default());
        assert!(!c.step(1));
        assert_eq!(c.layout, "");
    }
}
