// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod aa_events;
pub mod apply;
pub mod config;
pub mod features;
pub mod frame;
pub mod gnss;
pub mod helper;
pub mod helper_events;
pub mod helper_requests;
pub mod icon;
pub mod ids;
pub mod input;
pub mod message;
pub mod module;
pub mod patch;
pub mod runtime;
pub mod state;
pub mod telemetry;
#[cfg(test)]
mod telemetry_doc;
pub mod theme;

pub const PROTOCOL: u32 = 11;

pub const LIFELINE_ENV: &str = "LIVI_LIFELINE";
