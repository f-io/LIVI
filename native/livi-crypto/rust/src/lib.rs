//! ChaCha20-Poly1305 (RFC 8439) N-API addon for CarPlay frame crypto.
//!
//! Exports `seal`/`open`, loaded in-process via `require('livi-crypto')`.
//! The AEAD comes from aws-lc-rs (assembly ChaCha20/Poly1305, NEON on aarch64).

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
#[cfg(feature = "node")]
use napi::bindgen_prelude::Buffer;
#[cfg(feature = "node")]
use napi_derive::napi;

const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

fn key(raw: &[u8]) -> Option<LessSafeKey> {
    if raw.len() != KEY_LEN {
        return None;
    }
    UnboundKey::new(&CHACHA20_POLY1305, raw).ok().map(LessSafeKey::new)
}

fn nonce(raw: &[u8]) -> Option<Nonce> {
    let n: [u8; NONCE_LEN] = raw.try_into().ok()?;
    Some(Nonce::assume_unique_for_key(n))
}

pub fn seal_impl(key_raw: &[u8], nonce_raw: &[u8], pt: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    let k = key(key_raw)?;
    let n = nonce(nonce_raw)?;
    let mut buf = Vec::with_capacity(pt.len() + TAG_LEN);
    buf.extend_from_slice(pt);
    k.seal_in_place_append_tag(n, Aad::from(aad), &mut buf).ok()?;
    Some(buf)
}

pub fn open_impl(key_raw: &[u8], nonce_raw: &[u8], ct: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    if ct.len() < TAG_LEN {
        return None;
    }
    let k = key(key_raw)?;
    let n = nonce(nonce_raw)?;
    let mut buf = ct.to_vec();
    let pt_len = k.open_in_place(n, Aad::from(aad), &mut buf).ok()?.len();
    buf.truncate(pt_len);
    Some(buf)
}

/// seal(key: Buffer(32), nonce: Buffer(12), pt: Buffer, aad?: Buffer)
///   -> Buffer(ciphertext + 16-byte tag). Throws on bad key/nonce sizes.
#[cfg(feature = "node")]
#[napi]
pub fn seal(key: Buffer, nonce: Buffer, pt: Buffer, aad: Option<Buffer>) -> napi::Result<Buffer> {
    seal_impl(&key, &nonce, &pt, aad.as_deref().unwrap_or(&[]))
        .map(Buffer::from)
        .ok_or_else(|| napi::Error::from_reason("seal: key must be 32 and nonce 12 bytes"))
}

/// open(key: Buffer(32), nonce: Buffer(12), ct: Buffer(>=16), aad?: Buffer)
///   -> Buffer(plaintext) on a valid tag, null on auth failure or bad arguments.
#[cfg(feature = "node")]
#[napi]
pub fn open(key: Buffer, nonce: Buffer, ct: Buffer, aad: Option<Buffer>) -> Option<Buffer> {
    open_impl(&key, &nonce, &ct, aad.as_deref().unwrap_or(&[])).map(Buffer::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // RFC 8439 §2.8.2 AEAD test vector.
    const RFC_KEY: &str = "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f";
    const RFC_NONCE: &str = "070000004041424344454647";
    const RFC_AAD: &str = "50515253c0c1c2c3c4c5c6c7";
    const RFC_PT: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you \
only one tip for the future, sunscreen would be it.";
    const RFC_CT_TAG: &str = "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6\
3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58\
fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd060\
0691";

    #[test]
    fn rfc8439_known_answer() {
        let ct = seal_impl(&hex(RFC_KEY), &hex(RFC_NONCE), RFC_PT, &hex(RFC_AAD)).unwrap();
        assert_eq!(ct, hex(RFC_CT_TAG));
        let pt = open_impl(&hex(RFC_KEY), &hex(RFC_NONCE), &ct, &hex(RFC_AAD)).unwrap();
        assert_eq!(pt, RFC_PT);
    }

    #[test]
    fn round_trip_without_aad() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let ct = seal_impl(&key, &nonce, b"secret", &[]).unwrap();
        assert_eq!(ct.len(), 6 + TAG_LEN);
        assert_eq!(open_impl(&key, &nonce, &ct, &[]).unwrap(), b"secret");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let mut ct = seal_impl(&key, &nonce, b"x", &[]).unwrap();
        ct[0] ^= 0xff;
        assert!(open_impl(&key, &nonce, &ct, &[]).is_none());
    }

    #[test]
    fn aad_mismatch_fails() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let ct = seal_impl(&key, &nonce, b"x", b"aad").unwrap();
        assert!(open_impl(&key, &nonce, &ct, b"bad").is_none());
    }

    #[test]
    fn bad_sizes_are_rejected() {
        assert!(seal_impl(&[0u8; 16], &[0u8; 12], b"x", &[]).is_none());
        assert!(seal_impl(&[0u8; 32], &[0u8; 8], b"x", &[]).is_none());
        assert!(open_impl(&[0u8; 32], &[0u8; 12], &[0u8; 8], &[]).is_none());
    }

    #[test]
    fn empty_plaintext_is_tag_only() {
        let key = [1u8; 32];
        let nonce = [2u8; 12];
        let ct = seal_impl(&key, &nonce, b"", &[]).unwrap();
        assert_eq!(ct.len(), TAG_LEN);
        assert_eq!(open_impl(&key, &nonce, &ct, &[]).unwrap(), b"");
    }
}
