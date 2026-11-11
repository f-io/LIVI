// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, PrivateKey, UnparsedPublicKey};
use aws_lc_rs::cipher::{AES_128, EncryptingKey, EncryptionContext, UnboundCipherKey};
use aws_lc_rs::digest::{self, Context};
use aws_lc_rs::hkdf::{HKDF_SHA512, KeyType, Salt};
use aws_lc_rs::signature::{self, ED25519, Ed25519KeyPair, KeyPair};

pub const TAG_LEN: usize = 16;

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    aws_lc_rs::rand::fill(&mut out).expect("the system random source is unavailable");
    out
}

pub struct ChaCha(LessSafeKey);

impl ChaCha {
    pub fn new(key: &[u8; 32]) -> Self {
        let key = UnboundKey::new(&CHACHA20_POLY1305, key).expect("32 bytes are a ChaCha20 key");
        Self(LessSafeKey::new(key))
    }

    pub fn seal(&self, nonce: &[u8; 12], plain: &[u8], aad: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(plain.len() + TAG_LEN);
        buf.extend_from_slice(plain);
        self.0
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(*nonce),
                Aad::from(aad),
                &mut buf,
            )
            .expect("sealing a buffer in memory cannot fail");
        buf
    }

    pub fn open(&self, nonce: &[u8; 12], sealed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
        let (body, tag) = sealed.split_at_checked(sealed.len().checked_sub(TAG_LEN)?)?;
        self.open_detached(nonce, body, tag, aad)
    }

    pub fn open_detached(
        &self,
        nonce: &[u8; 12],
        body: &[u8],
        tag: &[u8],
        aad: &[u8],
    ) -> Option<Vec<u8>> {
        let mut out = vec![0u8; body.len()];
        self.0
            .open_separate_gather(
                Nonce::assume_unique_for_key(*nonce),
                Aad::from(aad),
                body,
                tag,
                &mut out,
            )
            .ok()?;
        Some(out)
    }
}

pub fn chacha_seal(key: &[u8; 32], nonce: &[u8; 12], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    ChaCha::new(key).seal(nonce, plain, aad)
}

pub fn chacha_open(key: &[u8; 32], nonce: &[u8; 12], sealed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    ChaCha::new(key).open(nonce, sealed, aad)
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

pub struct X25519 {
    secret: PrivateKey,
    pub public: [u8; 32],
}

impl X25519 {
    pub fn generate() -> Self {
        let secret = PrivateKey::generate(&agreement::X25519)
            .expect("the system random source is unavailable");
        let public = secret
            .compute_public_key()
            .expect("an X25519 key has a public half")
            .as_ref()
            .try_into()
            .expect("an X25519 public key is 32 bytes");
        Self { secret, public }
    }

    pub fn shared(self, peer: &[u8; 32]) -> Option<[u8; 32]> {
        agreement::agree(
            &self.secret,
            UnparsedPublicKey::new(&agreement::X25519, peer),
            (),
            |shared| shared.try_into().map_err(|_| ()),
        )
        .ok()
    }
}

fn ed25519(secret: &[u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(secret).expect("32 bytes are an Ed25519 seed")
}

pub fn ed25519_public(secret: &[u8; 32]) -> [u8; 32] {
    ed25519(secret)
        .public_key()
        .as_ref()
        .try_into()
        .expect("an Ed25519 public key is 32 bytes")
}

pub fn ed25519_sign(secret: &[u8; 32], data: &[u8]) -> [u8; 64] {
    ed25519(secret)
        .sign(data)
        .as_ref()
        .try_into()
        .expect("an Ed25519 signature is 64 bytes")
}

pub fn ed25519_verify(public: &[u8], data: &[u8], sig: &[u8]) -> bool {
    signature::UnparsedPublicKey::new(&ED25519, public)
        .verify(data, sig)
        .is_ok()
}

struct Len(usize);

impl KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

pub fn hkdf_sha512(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Salt::new(HKDF_SHA512, salt)
        .extract(ikm)
        .expand(&[info], Len(out.len()))
        .and_then(|okm| okm.fill(&mut out))
        .expect("32 bytes are a valid HKDF-SHA512 output length");
    out
}

fn hash<const N: usize>(alg: &'static digest::Algorithm, parts: &[&[u8]]) -> [u8; N] {
    let mut ctx = Context::new(alg);
    for p in parts {
        ctx.update(p);
    }
    ctx.finish()
        .as_ref()
        .try_into()
        .expect("the digest has the length of its algorithm")
}

pub fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    hash(&digest::SHA1_FOR_LEGACY_USE_ONLY, parts)
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    hash(&digest::SHA256, parts)
}

pub fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    hash(&digest::SHA512, parts)
}

pub fn aes_ctr128(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    let key = UnboundCipherKey::new(&AES_128, key).expect("16 bytes are an AES-128 key");
    EncryptingKey::ctr(key)
        .and_then(|k| k.less_safe_encrypt(&mut out, EncryptionContext::Iv128((*iv).into())))
        .expect("AES-CTR takes any length");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

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
        let key = hex(RFC_KEY).try_into().unwrap();
        let nonce = hex(RFC_NONCE).try_into().unwrap();
        let ct = chacha_seal(&key, &nonce, RFC_PT, &hex(RFC_AAD));
        assert_eq!(ct, hex(RFC_CT_TAG));
        let pt = chacha_open(&key, &nonce, &ct, &hex(RFC_AAD)).unwrap();
        assert_eq!(pt, RFC_PT);
    }

    #[test]
    fn round_trip_without_aad() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let ct = chacha_seal(&key, &nonce, b"secret", &[]);
        assert_eq!(ct.len(), 6 + TAG_LEN);
        assert_eq!(chacha_open(&key, &nonce, &ct, &[]).unwrap(), b"secret");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let mut ct = chacha_seal(&key, &nonce, b"x", &[]);
        ct[0] ^= 0xff;
        assert!(chacha_open(&key, &nonce, &ct, &[]).is_none());
    }

    #[test]
    fn aad_mismatch_fails() {
        let key = [0x42u8; 32];
        let nonce = [0u8; 12];
        let ct = chacha_seal(&key, &nonce, b"x", b"aad");
        assert!(chacha_open(&key, &nonce, &ct, b"bad").is_none());
    }

    #[test]
    fn a_body_shorter_than_the_tag_is_rejected() {
        assert!(chacha_open(&[0u8; 32], &[0u8; 12], &[0u8; 8], &[]).is_none());
    }

    #[test]
    fn a_detached_tag_opens_like_an_appended_one() {
        let key = ChaCha::new(&[5u8; 32]);
        let sealed = key.seal(&nonce64(9), b"detached", b"ad");
        let (body, tag) = sealed.split_at(sealed.len() - TAG_LEN);
        assert_eq!(
            key.open_detached(&nonce64(9), body, tag, b"ad").as_deref(),
            Some(&b"detached"[..])
        );
        assert_eq!(key.open_detached(&nonce64(9), body, &tag[..8], b"ad"), None);
    }

    #[test]
    fn empty_plaintext_is_tag_only() {
        let key = [1u8; 32];
        let nonce = [2u8; 12];
        let ct = chacha_seal(&key, &nonce, b"", &[]);
        assert_eq!(ct.len(), TAG_LEN);
        assert_eq!(chacha_open(&key, &nonce, &ct, &[]).unwrap(), b"");
    }

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

    #[test]
    fn hkdf_sha512_matches_hmac_sha512() {
        assert_eq!(
            hkdf_sha512(b"ikm", b"salt", b"info").to_vec(),
            hex("f8666bd3fdd88840c947614272dd065c71c541b0de03ff738644dffc3facec64")
        );
        assert_eq!(
            hkdf_sha512(b"ikm", b"", b"info").to_vec(),
            hex("3a602ee52593d4f927d57495b18ea66a5f4cf7dfdb1a7ddaf22459db5d3da294")
        );
    }

    #[test]
    fn ed25519_matches_rfc8032_test_one() {
        let secret = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
            .try_into()
            .unwrap();
        assert_eq!(
            ed25519_public(&secret).to_vec(),
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        );
        assert_eq!(
            ed25519_sign(&secret, b"").to_vec(),
            hex(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
                 5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            )
        );
    }

    #[test]
    fn aes_ctr_matches_sp800_38a() {
        let key = hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap();
        let iv = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff").try_into().unwrap();
        assert_eq!(
            aes_ctr128(
                &key,
                &iv,
                &hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51")
            ),
            hex("874d6191b620e3261bef6864990db6ce9806f66b7970fdff8617187bb9fffdff")
        );
    }

    #[test]
    fn digests_match_known_answers() {
        assert_eq!(
            sha256(&[b"a", b"bc"]).to_vec(),
            hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            sha1(&[b"abc"]).to_vec(),
            hex("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
        assert_eq!(&sha512(&[b"abc"])[..4], &hex("ddaf35a1")[..]);
    }
}
