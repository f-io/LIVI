// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_video_player::{Player, UrlPlayer};

use crate::{Plane, UrlPlay};

impl Plane for Player {
    fn start(&self) {
        Player::start(self)
    }

    fn push(&self, nal: &[u8]) {
        Player::push(self, nal);
    }

    fn flush(&self) {
        Player::flush(self)
    }

    fn set_gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
        Player::set_gamma(self, gamma, contrast, r, g, b)
    }
}

impl UrlPlay for UrlPlayer {
    fn start(&self) {
        UrlPlayer::start(self)
    }

    fn set_rate(&self, rate: f64) {
        UrlPlayer::set_rate(self, rate)
    }

    fn seek(&self, seconds: f64) {
        UrlPlayer::seek(self, seconds)
    }

    fn set_muted(&self, muted: bool) {
        UrlPlayer::set_muted(self, muted)
    }

    fn status(&self) -> crate::UrlStatus {
        UrlPlayer::status(self)
    }
}
