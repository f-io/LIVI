// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(feature = "ab5301")]
mod ab5301;
#[cfg(feature = "ab5301")]
pub use ab5301::{MODEL, Watch, get, set, start, watch};
#[cfg(not(feature = "ab5301"))]
compile_error!("uart-module needs a driver feature, e.g. ab5301");
