// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;

use livi_video_player::{CameraPlayer, Player, UrlPlayer};
use livi_video_view::View;

use crate::{Plane, UrlPlay, UrlStatus};

pub struct Shown<V: View + 'static> {
    id: u32,
    view: Arc<V>,
}

impl<V: View + 'static> Shown<V> {
    pub fn new(id: u32, view: Option<Arc<V>>) -> Option<Self> {
        view.map(|view| Self { id, view })
    }
}

impl<V: View + 'static> Drop for Shown<V> {
    fn drop(&mut self) {
        livi_video_view::remove(self.id, &self.view);
    }
}

impl Plane for Shown<Player> {
    fn start(&self) {
        Player::start(&self.view)
    }

    fn push(&self, nal: &[u8]) {
        Player::push(&self.view, nal);
    }

    fn flush(&self) {
        Player::flush(&self.view)
    }

    fn set_gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
        Player::set_gamma(&self.view, gamma, contrast, r, g, b)
    }
}

impl UrlPlay for Shown<UrlPlayer> {
    fn start(&self) {
        UrlPlayer::start(&self.view)
    }

    fn set_rate(&self, rate: f64) {
        UrlPlayer::set_rate(&self.view, rate)
    }

    fn seek(&self, seconds: f64) {
        UrlPlayer::seek(&self.view, seconds)
    }

    fn set_muted(&self, muted: bool) {
        UrlPlayer::set_muted(&self.view, muted)
    }

    fn status(&self) -> UrlStatus {
        UrlPlayer::status(&self.view)
    }
}

impl Shown<CameraPlayer> {
    pub fn start(&self) {
        CameraPlayer::start(&self.view)
    }
}
