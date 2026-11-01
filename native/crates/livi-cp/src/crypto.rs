// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use aes::Aes128;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use x25519_dalek::{EphemeralSecret, PublicKey};

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).expect("the system random source is unavailable");
    out
}

pub struct X25519 {
    secret: EphemeralSecret,
    pub public: [u8; 32],
}

impl X25519 {
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random();
        let public = PublicKey::from(&secret).to_bytes();
        Self { secret, public }
    }

    pub fn shared(self, peer: &[u8; 32]) -> Option<[u8; 32]> {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*peer));
        shared.was_contributory().then(|| shared.to_bytes())
    }
}

pub fn ed25519_public(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

pub fn ed25519_sign(secret: &[u8; 32], data: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(secret).sign(data).to_bytes()
}

pub fn ed25519_verify(public: &[u8], data: &[u8], sig: &[u8]) -> bool {
    let (Ok(public), Ok(sig)) = (<&[u8; 32]>::try_from(public), Signature::from_slice(sig)) else {
        return false;
    };
    VerifyingKey::from_bytes(public).is_ok_and(|key| key.verify(data, &sig).is_ok())
}

pub fn hkdf_sha512(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt), ikm)
        .expand(info, &mut out)
        .expect("32 bytes are a valid HKDF-SHA512 output length");
    out
}

pub fn chacha_seal(key: &[u8; 32], nonce: &[u8; 12], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    ChaCha20Poly1305::new(&(*key).into())
        .encrypt(&Nonce::from(*nonce), Payload { msg: plain, aad })
        .expect("sealing a buffer in memory cannot fail")
}

pub fn chacha_open(key: &[u8; 32], nonce: &[u8; 12], sealed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    ChaCha20Poly1305::new(&(*key).into())
        .decrypt(&Nonce::from(*nonce), Payload { msg: sealed, aad })
        .ok()
}

pub fn nonce64(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_le_bytes());
    n
}

pub fn nonce_label(label: &str) -> [u8; 12] {
    let mut n = [0u8; 12];
    let bytes = label.as_bytes();
    let len = bytes.len().min(8);
    n[4..4 + len].copy_from_slice(&bytes[..len]);
    n
}

pub fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn aes_ctr128(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    Ctr128BE::<Aes128>::new(&(*key).into(), &(*iv).into()).apply_keystream(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x25519_agrees_on_both_sides() {
        let a = X25519::generate();
        let b = X25519::generate();
        let (pa, pb) = (a.public, b.public);
        assert_eq!(a.shared(&pb), b.shared(&pa));
    }

    #[test]
    fn x25519_refuses_a_low_order_peer() {
        assert_eq!(X25519::generate().shared(&[0u8; 32]), None);
    }

    #[test]
    fn ed25519_signs_and_verifies() {
        let secret = random_bytes::<32>();
        let public = ed25519_public(&secret);
        let sig = ed25519_sign(&secret, b"data");
        assert!(ed25519_verify(&public, b"data", &sig));
        assert!(!ed25519_verify(&public, b"other", &sig));
        assert!(!ed25519_verify(&public[..31], b"data", &sig));
        assert!(!ed25519_verify(&public, b"data", &sig[..63]));
    }

    #[test]
    fn chacha_round_trip_and_tamper() {
        let key = [3u8; 32];
        let sealed = chacha_seal(&key, &nonce64(1), b"plain", b"ad");
        assert_eq!(sealed.len(), 5 + 16);
        assert_eq!(
            chacha_open(&key, &nonce64(1), &sealed, b"ad").as_deref(),
            Some(&b"plain"[..])
        );
        assert_eq!(chacha_open(&key, &nonce64(2), &sealed, b"ad"), None);
        assert_eq!(chacha_open(&key, &nonce64(1), &sealed, b"xx"), None);
    }

    #[test]
    fn nonces_are_right_aligned() {
        assert_eq!(nonce64(0x0102), [0, 0, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&nonce_label("PV-Msg02")[4..], b"PV-Msg02");
        assert_eq!(&nonce_label("PV-Msg02")[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn aes_ctr_is_its_own_inverse() {
        let enc = aes_ctr128(&[1; 16], &[2; 16], b"signature bytes!!");
        assert_ne!(enc, b"signature bytes!!");
        assert_eq!(aes_ctr128(&[1; 16], &[2; 16], &enc), b"signature bytes!!");
    }
}
