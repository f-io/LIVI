// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod audio;
pub mod input;
pub mod media_info;
pub mod mic;
pub mod nav_maneuver;
pub mod navigation;
pub mod video;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub ch: u8,
    pub flags: u8,
    pub msg_id: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(ch: u8, flags: u8, msg_id: u16, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            ch,
            flags,
            msg_id,
            payload: payload.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Emit<E> {
    Send(Frame),
    Event(E),
}

pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
