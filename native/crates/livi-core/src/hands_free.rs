// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::time::Instant;

pub const KEEP_EVERY: Duration = Duration::from_secs(30);
const NUDGE_GAP: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq)]
pub enum Io {
    Call(bool),
    WiredPhones(Vec<String>),
    Nudge(String),
    FindPhones,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Back {
    Phones(Vec<String>),
}

#[derive(Default)]
pub struct HandsFree {
    pub bt_by_instance: HashMap<String, String>,
    pub serial_by_instance: HashMap<String, String>,
    link_up: bool,
    links: HashMap<String, bool>,
    kept: HashSet<String>,
    nudged: HashMap<String, Instant>,
    pub battery_precise: bool,
    pub call: bool,
    wired: Vec<String>,
}

impl HandsFree {
    pub fn on_link(&mut self, up: bool, mac: Option<&str>) {
        self.link_up = up;
        if let Some(mac) = mac {
            self.log_link(&mac.to_lowercase(), up, "helper");
        }
    }

    fn log_link(&mut self, mac: &str, up: bool, by: &str) {
        if self.links.insert(mac.to_string(), up) != Some(up) {
            println!(
                "[core] HFP link {mac}: {} ({by})",
                if up { "up" } else { "down" }
            );
        }
    }

    pub fn keep(&mut self, mac: &str) -> bool {
        self.kept.insert(mac.to_lowercase())
    }

    pub fn kept(&self) -> Vec<String> {
        self.kept.iter().cloned().collect()
    }

    pub fn check(&mut self, mac: &str, wanted: bool, now: Instant) -> Option<Io> {
        if !wanted {
            self.kept.remove(mac);
            self.links.remove(mac);
            self.nudged.remove(mac);
            return None;
        }
        let up = self.link_up;
        self.log_link(mac, up, "phone-managed");
        if up || self.call {
            return None;
        }
        if self
            .nudged
            .get(mac)
            .is_some_and(|at| now.duration_since(*at) < NUDGE_GAP)
        {
            return None;
        }
        self.nudged.insert(mac.to_string(), now);
        Some(Io::Nudge(mac.to_string()))
    }

    pub fn wired(&mut self, ids: impl IntoIterator<Item = String>, again: bool) -> Option<Io> {
        let mut list: Vec<String> = ids.into_iter().map(|id| id.to_uppercase()).collect();
        list.sort();
        list.dedup();
        if !again && self.wired == list {
            return None;
        }
        self.wired = list.clone();
        Some(Io::WiredPhones(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_link_is_asked_up_again_at_the_phones_pace() {
        let mut hf = HandsFree::default();
        let t0 = Instant::now();
        assert!(hf.keep("AA:00:00:00:00:01"));
        assert!(!hf.keep("aa:00:00:00:00:01"));
        let mac = "aa:00:00:00:00:01";
        assert_eq!(hf.check(mac, true, t0), Some(Io::Nudge(mac.into())));
        assert_eq!(hf.check(mac, true, t0 + KEEP_EVERY), None);
        assert_eq!(
            hf.check(mac, true, t0 + NUDGE_GAP),
            Some(Io::Nudge(mac.into()))
        );

        hf.on_link(true, Some("AA:00:00:00:00:01"));
        assert_eq!(hf.check(mac, true, t0 + NUDGE_GAP * 3), None);
        hf.on_link(false, None);
        hf.call = true;
        assert_eq!(hf.check(mac, true, t0 + NUDGE_GAP * 3), None);
        hf.call = false;
        assert_eq!(
            hf.check(mac, true, t0 + NUDGE_GAP * 3),
            Some(Io::Nudge(mac.into()))
        );

        assert_eq!(hf.check(mac, false, t0 + NUDGE_GAP * 9), None);
        assert!(hf.kept().is_empty());
        assert!(hf.keep(mac));
    }

    #[test]
    fn the_wired_list_goes_out_when_it_changed_or_the_helper_came_back() {
        let mut hf = HandsFree::default();
        assert_eq!(hf.wired(Vec::new(), false), None);
        let list = || ["serial1".to_string(), "aa:bb".into(), "SERIAL1".into()];
        let sent = Io::WiredPhones(vec!["AA:BB".into(), "SERIAL1".into()]);
        assert_eq!(hf.wired(list(), false), Some(sent.clone()));
        assert_eq!(hf.wired(list(), false), None);
        assert_eq!(hf.wired(list(), true), Some(sent));
        assert_eq!(
            hf.wired(Vec::new(), false),
            Some(Io::WiredPhones(Vec::new()))
        );
    }
}
