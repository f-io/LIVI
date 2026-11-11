// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone)]
pub struct PhoneInfo {
    pub serial: String,
    pub num_configs: u8,
    pub config_value: Option<u8>,
}
