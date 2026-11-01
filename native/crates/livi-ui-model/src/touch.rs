// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::input::{Phase, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Down,
    Move,
    Up,
}

struct Finger {
    finger: u64,
    slot: u32,
    x: f64,
    y: f64,
}

#[derive(Default)]
pub struct Fingers {
    active: Vec<Finger>,
    free: Vec<u32>,
    next: u32,
    moved: bool,
}

impl Fingers {
    fn frame(&self, changed: Option<(u32, Phase)>) -> Vec<Point> {
        self.active
            .iter()
            .map(|f| Point {
                id: f.slot,
                x: f.x,
                y: f.y,
                phase: match changed {
                    Some((slot, phase)) if slot == f.slot => phase,
                    _ => Phase::Move,
                },
            })
            .collect()
    }

    pub fn touch(
        &mut self,
        finger: u64,
        at: Option<(f64, f64)>,
        change: Change,
    ) -> Option<Vec<Point>> {
        let known = self.active.iter().position(|f| f.finger == finger);
        match (change, known, at) {
            (Change::Down, Some(i), Some((x, y))) => {
                let f = &mut self.active[i];
                (f.x, f.y) = (x, y);
                let slot = f.slot;
                self.moved = false;
                Some(self.frame(Some((slot, Phase::Down))))
            }
            (Change::Down, None, Some((x, y))) => {
                let slot = self.free.pop().unwrap_or_else(|| {
                    self.next += 1;
                    self.next - 1
                });
                self.active.push(Finger { finger, slot, x, y });
                self.moved = false;
                Some(self.frame(Some((slot, Phase::Down))))
            }
            (Change::Move, Some(i), Some((x, y))) => {
                (self.active[i].x, self.active[i].y) = (x, y);
                self.moved = true;
                None
            }
            (Change::Up, Some(i), at) => {
                if let Some((x, y)) = at {
                    (self.active[i].x, self.active[i].y) = (x, y);
                }
                let slot = self.active[i].slot;
                let out = self.frame(Some((slot, Phase::Up)));
                self.active.remove(i);
                self.free.push(slot);
                self.moved = false;
                Some(out)
            }
            _ => None,
        }
    }

    pub fn flush(&mut self) -> Option<Vec<Point>> {
        std::mem::take(&mut self.moved).then(|| self.frame(None))
    }

    pub fn holds(&self, finger: u64) -> bool {
        self.active.iter().any(|f| f.finger == finger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(points: &[Point]) -> Vec<(u32, Phase)> {
        points.iter().map(|p| (p.id, p.phase)).collect()
    }

    #[test]
    fn every_change_sends_all_fingers_and_marks_the_changed_one() {
        let mut f = Fingers::default();
        let a = f.touch(70, Some((0.1, 0.1)), Change::Down).unwrap();
        assert_eq!(ids(&a), vec![(0, Phase::Down)]);
        let b = f.touch(71, Some((0.5, 0.5)), Change::Down).unwrap();
        assert_eq!(ids(&b), vec![(0, Phase::Move), (1, Phase::Down)]);
        assert!(f.touch(70, Some((0.2, 0.2)), Change::Move).is_none());
        let m = f.flush().unwrap();
        assert_eq!(m[0].x, 0.2);
        assert!(f.flush().is_none());
        let up = f.touch(70, None, Change::Up).unwrap();
        assert_eq!(ids(&up), vec![(0, Phase::Up), (1, Phase::Move)]);
        assert_eq!(up[0].x, 0.2);
        assert!(!f.holds(70));
    }

    #[test]
    fn a_freed_slot_is_used_again() {
        let mut f = Fingers::default();
        f.touch(1, Some((0.0, 0.0)), Change::Down);
        f.touch(2, Some((0.0, 0.0)), Change::Down);
        f.touch(1, None, Change::Up);
        let c = f.touch(3, Some((0.0, 0.0)), Change::Down).unwrap();
        assert_eq!(ids(&c), vec![(1, Phase::Move), (0, Phase::Down)]);
    }

    #[test]
    fn a_touch_that_starts_off_the_picture_is_ignored() {
        let mut f = Fingers::default();
        assert!(f.touch(9, None, Change::Down).is_none());
        assert!(f.touch(9, Some((0.5, 0.5)), Change::Move).is_none());
        assert!(f.touch(9, None, Change::Up).is_none());
    }
}
