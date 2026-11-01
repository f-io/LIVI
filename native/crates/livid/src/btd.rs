// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn run(_args: Vec<String>) -> i32 {
    crate::exit_rc(livi_btd::run())
}
