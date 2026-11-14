// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_video_player::{CameraPlayer, Player, UrlPlayer};

use crate::shown::Shown;
use crate::{CameraOrder, VideoCodec};

pub type Plane = Shown<Player>;
pub type Url = Shown<UrlPlayer>;
pub type Camera = Shown<CameraPlayer>;

pub fn create_plane(id: u32, codec: VideoCodec, codec_data: &[u8]) -> Option<Plane> {
    Shown::new(id, livi_video_view::create(id, codec, codec_data))
}

pub fn open_camera(id: u32, order: &CameraOrder) -> Option<Camera> {
    let camera = Shown::new(
        id,
        livi_video_view::camera(id, order.clone(), livi_video_player::to_stderr()),
    )?;
    camera.start();
    Some(camera)
}

pub fn play_url(id: u32, url: &str, audio_device: &str) -> Option<Url> {
    Shown::new(
        id,
        livi_video_view::play(id, url, audio_device, livi_video_player::to_stderr()),
    )
}
