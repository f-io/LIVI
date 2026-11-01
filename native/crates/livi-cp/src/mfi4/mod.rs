// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod cert;
pub mod keys;
pub mod session;
pub mod wire;

use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::sign::Signer;

pub use session::Session;

use crate::crypto::hkdf_sha512;

pub const AUTH_TYPE: u32 = 0x20;

pub fn transcript_proofs(shared: &[u8; 32], transcript: &[u8]) -> ([u8; 32], [u8; 32]) {
    let proof = |salt: &[u8], info: &[u8]| {
        let key = PKey::hmac(&hkdf_sha512(shared, salt, info)).expect("an HMAC key");
        let mut signer = Signer::new(MessageDigest::sha256(), &key).expect("HMAC-SHA256");
        signer.update(transcript).expect("HMAC update");
        signer
            .sign_to_vec()
            .expect("HMAC")
            .try_into()
            .expect("32 bytes")
    };
    (
        proof(b"MFi4-KT1-Salt", b"MFi4-KT1-Info"),
        proof(b"MFi4-KT2-Salt", b"MFi4-KT2-Info"),
    )
}
