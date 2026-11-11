// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the channel list needs linux")]
    NeedsLinux,
    #[error("netlink {what}: {source}")]
    Netlink {
        what: &'static str,
        source: io::Error,
    },
    #[error("nl80211 is not registered with generic netlink")]
    NoNl80211,
    #[error("no regulatory domain")]
    NoRegulatoryDomain,
    #[error("truncated netlink message")]
    Truncated,
    #[error("netlink error {0}")]
    Kernel(i32),
}
