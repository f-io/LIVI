// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const CREATE: u8 = 1;
pub const FRAME: u8 = 2;
pub const FLUSH: u8 = 3;
pub const PLAY_URL: u8 = 4;
pub const PLAY_RATE: u8 = 5;
pub const PLAY_SEEK: u8 = 6;
pub const PLAY_MUTE: u8 = 7;
pub const STATUS: u8 = 8;
pub const LOG: u8 = 9;
pub const CAMERA: u8 = 10;

pub const PATH_ENV: &str = "LIVI_UI_PLANES";
