// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;

use crate::hci;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("tunnel: {0}")]
    Tunnel(io::Error),
    #[error("no address")]
    NoAddress,
    #[error("the controller named no buffer sizes")]
    NoBufferSizes,
    #[error("the dongle is gone")]
    Gone,
    #[error("no answer in time")]
    Timeout,
    #[error("{opcode:#06x}: {source}")]
    Unanswered { opcode: u16, source: Box<Error> },
    #[error("{opcode:#06x} refused: {}", hci::reason(*status))]
    Refused { opcode: u16, status: u8 },
    #[error("{}", hci::reason(*.0))]
    PageFailed(u8),
    #[error("the link went")]
    LinkGone,
    #[error("authentication failed: {}", hci::reason(*.0))]
    AuthenticationFailed(u8),
    #[error("encryption failed: {}", hci::reason(*.0))]
    EncryptionFailed(u8),
    #[error("the phone turned channel {0} down")]
    ChannelRefused(u16),
    #[error("the phone offers no such service")]
    NoSuchService,
    #[error("the phone refused RFCOMM")]
    RfcommRefused,
    #[error("the session went")]
    SessionGone,
}
