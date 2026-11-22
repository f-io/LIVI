// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! Files under /tmp that more than one program reads or writes.

pub const BT_MAC: &str = "/tmp/livi/bt-mac";
pub const GPS_CLOCK: &str = "/tmp/livi-gps-clock";
pub const LED_CONFIG: &str = "/tmp/livi/led.toml";
pub const LED_PID: &str = "/tmp/livi/livi-ledd.pid";
pub const LED_DIR: &str = "/tmp/livi/led";
pub const HOSTAPD_BASE: &str = "/tmp/livi/hostapd.conf.saved";
pub const HOSTAPD_LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
pub const UPDATE_CONF: &str = "/tmp/livi/update.conf";
