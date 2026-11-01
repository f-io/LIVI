// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl VehicleState {
    pub(super) fn settle(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        self.keep_zones_synced(written, changed);
        self.switch_by_level(written, changed);
        self.follow_auto(written, changed);
        self.run_max_ac(written, changed);
        self.mute_by_volume(written, changed);
    }

    pub(super) fn mute_by_volume(
        &mut self,
        written: &[(Value, Value)],
        changed: &mut Vec<(Value, Value)>,
    ) {
        let find = |iid| {
            written
                .iter()
                .find(|(k, _)| k.as_int() == Some(iid))
                .map(|(_, v)| v)
        };
        let level = self
            .values
            .get(&VOLUME)
            .and_then(Value::as_int)
            .unwrap_or(0);
        match (find(MUTE), find(VOLUME)) {
            (Some(Value::Bool(true)), _) if level > 0 => {
                self.resume.insert(VOLUME, Value::Int(level));
                self.car_sets(VOLUME, Value::Int(0), changed);
            }
            (Some(Value::Bool(false)), _) if level == 0 => {
                let back = self.resume.get(&VOLUME).cloned().unwrap_or(Value::Int(50));
                self.car_sets(VOLUME, back, changed);
            }
            (None, Some(Value::Int(v))) if *v > 0 => {
                self.car_sets(MUTE, Value::Bool(false), changed)
            }
            _ => {}
        }
    }

    pub(super) fn keep_zones_synced(
        &mut self,
        written: &[(Value, Value)],
        changed: &mut Vec<(Value, Value)>,
    ) {
        if self.values.get(&ZONES_SYNCED) != Some(&Value::Bool(true)) {
            return;
        }
        let wrote = |iid| written.iter().any(|(k, _)| k.as_int() == Some(iid));
        if wrote(PASSENGER_TARGET_TEMPERATURE) && !wrote(ZONES_SYNCED) {
            self.car_sets(ZONES_SYNCED, Value::Bool(false), changed);
        } else if let Some(driver) = self.values.get(&DRIVER_TARGET_TEMPERATURE).cloned() {
            self.car_sets(PASSENGER_TARGET_TEMPERATURE, driver, changed);
        }
    }

    pub(super) fn switch_by_level(
        &mut self,
        written: &[(Value, Value)],
        changed: &mut Vec<(Value, Value)>,
    ) {
        let find = |iid| {
            written
                .iter()
                .find(|(k, _)| k.as_int() == Some(iid))
                .map(|(_, v)| v)
        };
        for (level_iid, on_iid) in LEVELLED {
            if let Some(Value::Int(level)) = find(level_iid) {
                self.car_sets(on_iid, Value::Bool(*level > 0), changed);
            } else if let Some(Value::Bool(on)) = find(on_iid) {
                let level = self.values.get(&level_iid).cloned();
                if !on && level != Some(Value::Int(0)) {
                    if let Some(level) = level {
                        self.resume.insert(level_iid, level);
                    }
                    self.car_sets(level_iid, Value::Int(0), changed);
                } else if *on && level == Some(Value::Int(0)) {
                    let back = self
                        .resume
                        .get(&level_iid)
                        .cloned()
                        .unwrap_or(Value::Int(1));
                    self.car_sets(level_iid, back, changed);
                }
            }
        }
    }

    pub(super) fn follow_auto(
        &mut self,
        written: &[(Value, Value)],
        changed: &mut Vec<(Value, Value)>,
    ) {
        let find = |iid| {
            written
                .iter()
                .find(|(k, _)| k.as_int() == Some(iid))
                .map(|(_, v)| v)
        };
        let vents_auto = VENTS.map(|vent| vent + 2);
        if let Some(Value::Bool(auto)) = find(AUTO_CLIMATE) {
            for iid in std::iter::once(FAN_AUTO).chain(vents_auto) {
                self.car_sets(iid, Value::Bool(*auto), changed);
            }
            return;
        }
        let (fan_level, fan_on) = FAN;
        let fan_by_hand = find(fan_level).or(find(fan_on)).is_some();
        let vents_by_hand = VENTS.iter().any(|vent| find(vent + 1).is_some());
        if fan_by_hand {
            self.car_sets(FAN_AUTO, Value::Bool(false), changed);
        }
        if vents_by_hand {
            for iid in vents_auto {
                self.car_sets(iid, Value::Bool(false), changed);
            }
        }
        if fan_by_hand || vents_by_hand {
            self.car_sets(AUTO_CLIMATE, Value::Bool(false), changed);
        }
    }

    pub(super) fn run_max_ac(
        &mut self,
        written: &[(Value, Value)],
        changed: &mut Vec<(Value, Value)>,
    ) {
        let (fan_level, fan_on) = FAN;
        let lo = Value::Real(TEMPERATURE_LO);
        let takes = [
            (DRIVER_TARGET_TEMPERATURE, lo.clone()),
            (PASSENGER_TARGET_TEMPERATURE, lo),
            (COMPRESSOR, Value::Bool(true)),
            (RECIRCULATION, Value::Bool(true)),
            (fan_level, Value::Int(7)),
            (fan_on, Value::Bool(true)),
            (FAN_AUTO, Value::Bool(false)),
            (AUTO_CLIMATE, Value::Bool(false)),
        ];
        let find = |iid| {
            written
                .iter()
                .find(|(k, _)| k.as_int() == Some(iid))
                .map(|(_, v)| v)
        };
        match find(MAX_AC) {
            Some(Value::Bool(true)) if self.before_max_ac.is_none() => {
                let before = takes
                    .iter()
                    .filter_map(|(iid, _)| Some((*iid, self.values.get(iid)?.clone())));
                self.before_max_ac = Some(before.collect());
                for (iid, v) in takes {
                    self.car_sets(iid, v, changed);
                }
            }
            Some(Value::Bool(false)) => {
                for (iid, v) in self.before_max_ac.take().unwrap_or_default() {
                    self.car_sets(iid, v, changed);
                }
            }
            _ if self.before_max_ac.is_some()
                && takes.iter().any(|(iid, _)| find(*iid).is_some()) =>
            {
                self.before_max_ac = None;
                self.car_sets(MAX_AC, Value::Bool(false), changed);
            }
            _ => {}
        }
    }

    pub(super) fn car_sets(&mut self, iid: i64, v: Value, changed: &mut Vec<(Value, Value)>) {
        if self.values.get(&iid) == Some(&v) {
            return;
        }
        if let Some(c) = self.characteristic(iid) {
            println!("[cpVehicle] car set {} to {}", c.name, shown(&v));
        }
        self.values.insert(iid, v.clone());
        if self.followed.contains(&iid) {
            changed.push((Value::Int(iid), v));
        }
    }
}
