// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::ui_plane::{UiCamera, UiPlane, UiUrlPlayer};
use crate::{CameraOrder, VideoCodec};

pub type Plane = UiPlane;
pub type Url = UiUrlPlayer;
pub type Camera = UiCamera;

pub fn create_plane(id: u32, codec: VideoCodec, codec_data: &[u8]) -> Option<Plane> {
    UiPlane::open(&ui_planes_path(id)?, id, codec, codec_data)
}

pub fn open_camera(id: u32, order: &CameraOrder) -> Option<Camera> {
    UiCamera::open(&ui_planes_path(id)?, id, order)
}

pub fn play_url(id: u32, url: &str, audio_device: &str) -> Option<Url> {
    UiUrlPlayer::open(&ui_planes_path(id)?, id, url, audio_device)
}

fn ui_planes_path(id: u32) -> Option<String> {
    std::env::var(livi_output_proto::ui_planes::PATH_ENV)
        .inspect_err(|_| eprintln!("[output] no UI to draw plane 0x{id:x} in"))
        .ok()
}
