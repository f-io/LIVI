// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Player {
    Phone,
    Radio,
}

#[derive(Debug, Default)]
pub(super) struct Players {
    phone: bool,
    radio: bool,
}

impl Players {
    pub(super) fn superseded(&mut self, phone: bool, radio: bool) -> Option<Player> {
        let phone_started = phone && !self.phone;
        let radio_started = radio && !self.radio;
        self.phone = phone;
        self.radio = radio;
        match (phone_started, radio_started) {
            (true, false) if radio => Some(Player::Radio),
            (false, true) if phone => Some(Player::Phone),
            _ => None,
        }
    }
}

impl Projection {
    pub(super) fn follow_players(&mut self, phone: bool, radio: bool) {
        match self.players.superseded(phone, radio) {
            Some(Player::Radio) => {
                if let Some(asks) = &self.radio {
                    println!("[core] the phone plays, the radio stops");
                    let _ = asks.send(crate::radio::RadioAsk::Stop);
                }
            }
            Some(Player::Phone) => {
                println!("[core] the radio plays, the phone pauses");
                self.media(MediaControl::Pause);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_started_last_wins() {
        let mut p = Players::default();
        assert_eq!(p.superseded(true, false), None);
        assert_eq!(p.superseded(true, true), Some(Player::Phone));
        assert_eq!(p.superseded(false, true), None);
        assert_eq!(p.superseded(true, true), Some(Player::Radio));
        assert_eq!(p.superseded(true, false), None);
    }

    #[test]
    fn a_player_alone_stops_nothing() {
        let mut p = Players::default();
        assert_eq!(p.superseded(false, true), None);
        assert_eq!(p.superseded(false, false), None);
        assert_eq!(p.superseded(true, false), None);
    }

    #[test]
    fn both_starting_at_once_stops_neither() {
        let mut p = Players::default();
        assert_eq!(p.superseded(true, true), None);
    }
}
