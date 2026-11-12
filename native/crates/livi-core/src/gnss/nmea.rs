// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod decode;
mod encode;

pub use decode::Decoder;
pub use encode::{Position, encode};

fn checksum(body: &str) -> u8 {
    body.bytes().fold(0, |sum, b| sum ^ b)
}
