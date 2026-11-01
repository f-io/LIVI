// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const OP_CREATE: u8 = 1;
pub const OP_DATA: u8 = 2;
pub const OP_STOP: u8 = 3;
pub const OP_GAMMA: u8 = 4;
pub const OP_LISTEN: u8 = 5;
pub const OP_TEARDOWN: u8 = 6;
pub const OP_SET_ACTIVE: u8 = 7;
pub const OP_AUDIO_OPEN: u8 = 8;
pub const OP_AUDIO_VOLUME: u8 = 9;
pub const OP_AUDIO_STOP: u8 = 10;
pub const OP_MIC_OPEN: u8 = 11;
pub const OP_MIC_STOP: u8 = 12;
pub const OP_AUDIO_ACTIVE: u8 = 13;
pub const OP_AUDIO_DATA: u8 = 14;
pub const OP_VISUALIZER: u8 = 15;
pub const OP_FEED_OPEN: u8 = 16;
pub const OP_AUDIO_OUTPUT: u8 = 17;
pub const OP_FEED_ROUTE: u8 = 18;
pub const OP_FEED_CLOSE: u8 = 19;
pub const OP_TAP_OPEN: u8 = 20;
pub const OP_TAP_STOP: u8 = 21;
pub const OP_PLAY_URL: u8 = 22;
pub const OP_PLAY_RATE: u8 = 23;
pub const OP_PLAY_SEEK: u8 = 24;
pub const OP_PLAY_MUTE: u8 = 25;
pub const OP_CAMERA_OPEN: u8 = 26;
pub const OP_CAMERAS_WATCH: u8 = 27;

pub const REPLY_PORT: u8 = 1;
pub const REPLY_CONFIG: u8 = 2;
pub const REPLY_STARTED: u8 = 3;
pub const REPLY_AUDIO_PORTS: u8 = 4;
pub const REPLY_AUDIO_STARTED: u8 = 5;
pub const REPLY_VISUALIZER: u8 = 6;
pub const REPLY_FEED: u8 = 7;
pub const REPLY_URL_STATUS: u8 = 8;
pub const REPLY_AUDIO_DELAY: u8 = 9;
pub const REPLY_CAMERAS: u8 = 10;
