// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(any(target_os = "android", target_os = "linux"))]
pub mod responder;
#[cfg(any(target_os = "android", target_os = "linux", test))]
pub mod service;
pub mod wire;
