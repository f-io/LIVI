// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod cert;
pub mod keys;
pub mod session;
pub mod wire;

pub use session::Session;

use livi_crypto::{hkdf_sha512, hmac_sha256};

pub const AUTH_TYPE: u32 = 0x20;

pub fn transcript_proofs(shared: &[u8; 32], transcript: &[u8]) -> ([u8; 32], [u8; 32]) {
    let proof =
        |salt: &[u8], info: &[u8]| hmac_sha256(&hkdf_sha512(shared, salt, info), transcript);
    (
        proof(b"MFi4-KT1-Salt", b"MFi4-KT1-Info"),
        proof(b"MFi4-KT2-Salt", b"MFi4-KT2-Info"),
    )
}
