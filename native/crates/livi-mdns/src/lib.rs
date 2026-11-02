// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(target_os = "android")]
pub mod responder;
#[cfg(any(target_os = "android", test))]
pub mod service;
pub mod wire;
