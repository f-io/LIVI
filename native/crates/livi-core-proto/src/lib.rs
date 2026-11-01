// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod config;
pub mod frame;
pub mod gnss;
pub mod icon;
pub mod input;
pub mod message;
pub mod patch;
pub mod runtime;
pub mod state;
pub mod telemetry;
pub mod theme;

pub const PROTOCOL: u32 = 11;

pub const LIFELINE_ENV: &str = "LIVI_LIFELINE";
