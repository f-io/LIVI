// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const DEFAULT_120: &str = include_str!("../res/icon-120.b64");
pub const DEFAULT_180: &str = include_str!("../res/icon-180.b64");
pub const DEFAULT_256: &str = include_str!("../res/icon-256.b64");

pub const SIZES: [(&str, u32); 3] = [
    ("carplayIcon120", 120),
    ("carplayIcon180", 180),
    ("carplayIcon256", 256),
];
