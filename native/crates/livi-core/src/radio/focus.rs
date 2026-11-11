// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::files::status_file::Activity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Pause,
    Resume,
}

#[derive(Debug, Default)]
pub struct Focus {
    busy: bool,
    held: bool,
}

impl Focus {
    pub fn next(&mut self, now: Activity, playing: bool) -> Option<Turn> {
        let busy = now.phone || now.voice_assistant;
        let was = std::mem::replace(&mut self.busy, busy);
        if busy && !was && playing {
            self.held = true;
            return Some(Turn::Pause);
        }
        if self.held && !busy {
            self.held = false;
            return Some(Turn::Resume);
        }
        None
    }

    pub fn let_go(&mut self) {
        self.held = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> Activity {
        Activity {
            phone: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_call_holds_the_radio_until_it_ends() {
        let mut f = Focus::default();
        assert_eq!(f.next(call(), true), Some(Turn::Pause));
        assert_eq!(f.next(call(), false), None);
        assert_eq!(f.next(Activity::default(), false), Some(Turn::Resume));
        assert_eq!(f.next(Activity::default(), true), None);
    }

    #[test]
    fn siri_holds_the_radio_too() {
        let mut f = Focus::default();
        let siri = Activity {
            voice_assistant: true,
            ..Default::default()
        };
        assert_eq!(f.next(siri, true), Some(Turn::Pause));
        assert_eq!(f.next(Activity::default(), false), Some(Turn::Resume));
    }

    #[test]
    fn a_quiet_radio_stays_quiet() {
        let mut f = Focus::default();
        assert_eq!(f.next(call(), false), None);
        assert_eq!(f.next(Activity::default(), false), None);
    }

    #[test]
    fn the_user_choosing_during_a_call_wins() {
        let mut f = Focus::default();
        f.next(call(), true);
        f.let_go();
        assert_eq!(f.next(Activity::default(), false), None);
    }

    #[test]
    fn phone_music_and_nav_prompts_are_no_interruption() {
        let mut f = Focus::default();
        let other = Activity {
            media: true,
            nav_announcing: true,
            speech: true,
            ..Default::default()
        };
        assert_eq!(f.next(other, true), None);
    }
}
