// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::csm_message;

csm_message! {
    pub struct DeviceTimeUpdate = 0x4E0B {
        0 => seconds_since_reference_date: [opt i64],
        1 => time_zone_offset_minutes: [opt i16],
        2 => daylight_savings_offset_minutes: [opt i8],
    }
}
