// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::{DabStation, FmStation};

/// A station in an own list: DAB+ ones by service id, FM ones by frequency in kHz.
pub trait Listed {
    fn id(&self) -> u32;
    fn favorite(&self) -> bool;
    fn hidden(&self) -> bool;
    fn set_favorite(&mut self, on: bool);
    fn set_hidden(&mut self, on: bool);
}

impl Listed for DabStation {
    fn id(&self) -> u32 {
        self.service_id
    }
    fn favorite(&self) -> bool {
        self.favorite
    }
    fn hidden(&self) -> bool {
        self.hidden
    }
    fn set_favorite(&mut self, on: bool) {
        self.favorite = on;
    }
    fn set_hidden(&mut self, on: bool) {
        self.hidden = on;
    }
}

impl Listed for FmStation {
    fn id(&self) -> u32 {
        self.khz
    }
    fn favorite(&self) -> bool {
        self.favorite
    }
    fn hidden(&self) -> bool {
        self.hidden
    }
    fn set_favorite(&mut self, on: bool) {
        self.favorite = on;
    }
    fn set_hidden(&mut self, on: bool) {
        self.hidden = on;
    }
}

/// Stations found again keep their place and marks, new ones follow in `order`.
pub fn merged<T: Listed, K: Ord>(
    own: &[T],
    mut found: Vec<T>,
    order: impl FnMut(&T) -> K,
) -> Vec<T> {
    let mut out: Vec<T> = own
        .iter()
        .filter_map(|old| {
            let at = found.iter().position(|f| f.id() == old.id())?;
            let mut st = found.remove(at);
            st.set_favorite(old.favorite());
            st.set_hidden(old.hidden());
            Some(st)
        })
        .collect();
    found.sort_by_key(order);
    out.extend(found);
    out
}

pub fn step_target<T: Listed>(list: &[T], current: Option<u32>, by: i32) -> Option<u32> {
    let visible: Vec<&T> = list.iter().filter(|s| !s.hidden()).collect();
    if visible.is_empty() {
        return None;
    }
    let n = visible.len() as i64;
    if let Some(at) = visible.iter().position(|s| Some(s.id()) == current) {
        return Some(visible[(at as i64 + by as i64).rem_euclid(n) as usize].id());
    }
    let at = current.and_then(|id| list.iter().position(|s| s.id() == id));
    let Some(at) = at else {
        return Some(visible[0].id());
    };
    let len = list.len() as i64;
    let dir = if by < 0 { -1 } else { 1 };
    (1..=len)
        .map(|k| &list[(at as i64 + dir * k).rem_euclid(len) as usize])
        .find(|s| !s.hidden())
        .map(Listed::id)
}

pub fn move_station<T: Listed>(list: &mut [T], id: u32, by: i32) -> bool {
    let Some(at) = list.iter().position(|s| s.id() == id) else {
        return false;
    };
    let to = (at as i64 + by as i64).clamp(0, list.len() as i64 - 1) as usize;
    if to == at {
        return false;
    }
    if to > at {
        list[at..=to].rotate_left(1);
    } else {
        list[to..=at].rotate_right(1);
    }
    true
}

pub fn star<T: Listed>(list: &mut Vec<T>, id: u32, favorite: bool) -> bool {
    let Some(at) = list.iter().position(|s| s.id() == id) else {
        return false;
    };
    let mut st = list.remove(at);
    st.set_favorite(favorite);
    let to = list.iter().rposition(Listed::favorite).map_or(0, |i| i + 1);
    list.insert(to, st);
    true
}

pub fn hide<T: Listed>(list: &mut [T], id: u32, hidden: bool) -> bool {
    let Some(st) = list.iter_mut().find(|s| s.id() == id) else {
        return false;
    };
    st.set_hidden(hidden);
    true
}

pub fn hidden_last<T: Listed>(list: &mut [T]) {
    list.sort_by_key(Listed::hidden);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(id: u32, label: &str, favorite: bool) -> DabStation {
        DabStation {
            service_id: id,
            label: label.into(),
            short_label: String::new(),
            ensemble: "DR Deutschland".into(),
            channel: "5C".into(),
            frequency_hz: 1,
            favorite,
            hidden: false,
            logo: None,
        }
    }

    fn ids<T: Listed>(list: &[T]) -> Vec<u32> {
        list.iter().map(Listed::id).collect()
    }

    fn hidden(id: u32) -> DabStation {
        DabStation {
            hidden: true,
            ..named(id, "h", false)
        }
    }

    #[test]
    fn a_star_moves_the_station_behind_the_last_favorite() {
        let mut list = vec![
            named(1, "a", true),
            named(2, "b", false),
            named(3, "c", false),
            named(4, "d", false),
        ];
        assert!(star(&mut list, 4, true));
        assert_eq!(ids(&list), [1, 4, 2, 3]);
        assert!(list[1].favorite);
        assert!(star(&mut list, 1, false));
        assert_eq!(
            ids(&list),
            [4, 1, 2, 3],
            "unstarred it follows the remaining favorites"
        );
        assert!(!list[1].favorite);
        assert!(star(&mut list, 4, false));
        assert_eq!(
            ids(&list),
            [4, 1, 2, 3],
            "no favorites left, it goes to the top"
        );
        assert!(!star(&mut list, 9, true));
    }

    #[test]
    fn hidden_stations_sink_to_the_end() {
        let mut list = vec![
            named(1, "a", true),
            hidden(2),
            named(3, "c", false),
            hidden(4),
        ];
        list.push(named(5, "e", false));
        hidden_last(&mut list);
        assert_eq!(ids(&list), [1, 3, 5, 2, 4]);
        list[3].hidden = false;
        hidden_last(&mut list);
        assert_eq!(
            ids(&list),
            [1, 3, 5, 2, 4],
            "shown again it closes the visible ones"
        );
        assert!(hide(&mut list, 1, true));
        assert!(!hide(&mut list, 9, true));
        hidden_last(&mut list);
        assert_eq!(ids(&list), [3, 5, 2, 1, 4]);
    }

    #[test]
    fn steps_skip_hidden_stations() {
        let list = [
            named(1, "a", false),
            hidden(2),
            named(3, "c", false),
            hidden(4),
        ];
        assert_eq!(step_target(&list, Some(1), 1), Some(3));
        assert_eq!(
            step_target(&list, Some(3), 1),
            Some(1),
            "wraps past the hidden end"
        );
        assert_eq!(step_target(&list, Some(1), -1), Some(3));
        assert_eq!(
            step_target(&list, Some(2), 1),
            Some(3),
            "from a hidden one onwards"
        );
        assert_eq!(step_target(&list, Some(2), -1), Some(1));
        assert_eq!(step_target(&list, None, 1), Some(1));
        assert_eq!(step_target(&list, Some(9), 1), Some(1));
        assert_eq!(step_target(&[hidden(2)], Some(2), 1), None);
    }

    #[test]
    fn a_new_scan_keeps_own_order_and_favorites() {
        let own = [
            hidden(3),
            named(1, "a", true),
            DabStation {
                hidden: true,
                ..named(2, "b", false)
            },
        ];
        let found = vec![
            named(2, "b", false),
            named(1, "a new", false),
            named(5, "e", false),
            named(4, "d", false),
        ];
        let list = merged(&own, found, |s| s.label.to_lowercase());
        assert_eq!(
            ids(&list),
            [1, 2, 4, 5],
            "3 is gone, 4 and 5 come last by name"
        );
        assert!(list[0].favorite);
        assert_eq!(list[0].label, "a new");
        assert!(list[1].hidden, "hidden stays hidden");
        assert!(!list[2].hidden);
    }

    #[test]
    fn stations_move_within_the_own_order() {
        let mut list = vec![
            named(1, "a", false),
            named(2, "b", false),
            named(3, "c", false),
        ];
        assert!(move_station(&mut list, 1, 2));
        assert_eq!(ids(&list), [2, 3, 1]);
        assert!(move_station(&mut list, 1, -1));
        assert_eq!(ids(&list), [2, 1, 3]);
        assert!(!move_station(&mut list, 2, -1), "already at the top");
        assert!(!move_station(&mut list, 9, 1), "unknown station");
    }
}
