// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_crypto::{CCM_TAG_LEN, P256, aes128_ccm_open, aes128_ccm_seal, sha256};

pub use livi_crypto::aes128_cmac as cmac;

pub const POINT: usize = 33;
pub const TAG: usize = CCM_TAG_LEN;
const EXPANDED: usize = 200;
const EXTRACTION: &[u8] = b"MFi 4.0 SIGMA-I Authentication Randomness Extraction";
const EXPANSION: &[u8] = b"MFi 4.0 SIGMA-I Authentication Key Expansion";
pub const SIGMA: u8 = 0x01;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the phone's key is no point on P-256")]
    NotOnCurve,
}

pub struct Ephemeral {
    key: P256,
    pub public: [u8; POINT],
}

impl Ephemeral {
    pub fn generate() -> Self {
        let key = P256::generate();
        Self {
            public: key.public,
            key,
        }
    }

    pub fn shared(&self, peer: &[u8; POINT]) -> Result<Vec<u8>, KeyError> {
        Ok(self.key.shared(peer).ok_or(KeyError::NotOnCurve)?.to_vec())
    }
}

pub struct Channel {
    key: [u8; 16],
    nonce: [u8; 12],
}

impl Channel {
    fn new(material: &[u8], key_at: usize, nonce_at: usize) -> Self {
        Self {
            key: material[key_at..key_at + 16].try_into().expect("16 bytes"),
            nonce: material[nonce_at..nonce_at + 12]
                .try_into()
                .expect("12 bytes"),
        }
    }

    fn step(&mut self) {
        for byte in self.nonce.iter_mut().rev() {
            *byte = byte.wrapping_add(1);
            if *byte != 0 {
                break;
            }
        }
    }

    pub fn seal(&mut self, aad: &[u8], plain: &[u8]) -> Vec<u8> {
        let out = aes128_ccm_seal(&self.key, &self.nonce, aad, plain);
        self.step();
        out
    }

    pub fn open(&mut self, aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        let plain = aes128_ccm_open(&self.key, &self.nonce, aad, sealed)?;
        self.step();
        Some(plain)
    }
}

pub struct Keys {
    pub start: Channel,
    pub phone_cert: Channel,
    pub from_phone: Channel,
    pub finish: Channel,
    pub our_cert: Channel,
    pub ours: Channel,
    pub phone_mac: [u8; 16],
    pub our_mac: [u8; 16],
}

pub fn derive(shared: &[u8], context: &[u8]) -> Keys {
    let salt: [u8; 16] = sha256(&[EXTRACTION])[..16].try_into().expect("16 bytes");
    let kdk = cmac(&salt, shared);
    let bits = ((EXPANDED * 8) as u32).to_be_bytes();
    let mut material = Vec::with_capacity(EXPANDED + 16);
    let mut block = 1u32;
    while material.len() < EXPANDED {
        let input = [&block.to_be_bytes()[..], EXPANSION, &[0], context, &bits].concat();
        material.extend_from_slice(&cmac(&kdk, &input));
        block += 1;
    }
    let mac = |at: usize| -> [u8; 16] { material[at..at + 16].try_into().expect("16 bytes") };
    Keys {
        start: Channel::new(&material, 0, 16),
        phone_mac: mac(28),
        phone_cert: Channel::new(&material, 44, 60),
        from_phone: Channel::new(&material, 72, 88),
        finish: Channel::new(&material, 100, 116),
        our_mac: mac(128),
        our_cert: Channel::new(&material, 144, 160),
        ours: Channel::new(&material, 172, 188),
    }
}

pub fn signed_data(ours: &[u8; POINT], phone: &[u8; POINT], mac: &[u8; 16]) -> Vec<u8> {
    [&[SIGMA][..], ours, phone, mac].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmac_matches_the_published_aes_example() {
        let key: [u8; 16] = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let msg = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        assert_eq!(
            cmac(&key, &msg),
            [
                0x07, 0x0a, 0x16, 0xb4, 0x6b, 0x4d, 0x41, 0x44, 0xf7, 0x9b, 0xdd, 0x9d, 0xd0, 0x4a,
                0x28, 0x7c
            ]
        );
    }

    fn openssl_ccm_seal(key: &[u8; 16], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
        use openssl::cipher::Cipher;
        use openssl::cipher_ctx::CipherCtx;
        let mut ctx = CipherCtx::new().unwrap();
        ctx.encrypt_init(Some(Cipher::aes_128_ccm()), None, None)
            .unwrap();
        ctx.set_iv_length(nonce.len()).unwrap();
        ctx.set_tag_length(TAG).unwrap();
        ctx.encrypt_init(None, Some(key), Some(nonce)).unwrap();
        ctx.set_data_len(plain.len()).unwrap();
        if !aad.is_empty() {
            ctx.cipher_update(aad, None).unwrap();
        }
        let mut out = vec![0; plain.len() + 16];
        let mut n = ctx.cipher_update(plain, Some(&mut out)).unwrap();
        n += ctx.cipher_final(&mut out[n..]).unwrap();
        out.truncate(n);
        let mut tag = [0; TAG];
        ctx.tag(&mut tag).unwrap();
        out.extend_from_slice(&tag);
        out
    }

    #[test]
    fn ccm_seals_as_openssl_does() {
        for len in [0, 1, 16, 33, 64, 650] {
            let plain: Vec<u8> = (0..len).map(|i| i as u8).collect();
            for aad in [&b""[..], &[0xa5; 7][..], &[1; 40][..]] {
                let (key, nonce) = ([len as u8; 16], [aad.len() as u8; 12]);
                let mut ch = Channel { key, nonce };
                assert_eq!(
                    ch.seal(aad, &plain),
                    openssl_ccm_seal(&key, &nonce, aad, &plain)
                );
            }
        }
    }

    #[test]
    fn ecdh_agrees_with_openssl() {
        use openssl::bn::BigNumContext;
        use openssl::derive::Deriver;
        use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
        use openssl::nid::Nid;
        use openssl::pkey::PKey;
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let theirs = EcKey::generate(&group).unwrap();
        let mut ctx = BigNumContext::new().unwrap();
        let their_point: [u8; POINT] = theirs
            .public_key()
            .to_bytes(&group, PointConversionForm::COMPRESSED, &mut ctx)
            .unwrap()
            .try_into()
            .unwrap();
        let ours = Ephemeral::generate();
        let our_point = EcPoint::from_bytes(&group, &ours.public, &mut ctx).unwrap();
        let our_key =
            PKey::from_ec_key(EcKey::from_public_key(&group, &our_point).unwrap()).unwrap();
        let their_key = PKey::from_ec_key(theirs).unwrap();
        let mut deriver = Deriver::new(&their_key).unwrap();
        deriver.set_peer(&our_key).unwrap();
        assert_eq!(
            ours.shared(&their_point).unwrap(),
            deriver.derive_to_vec().unwrap()
        );
    }

    #[test]
    fn both_ends_agree_and_a_channel_counts_its_nonce() {
        let a = Ephemeral::generate();
        let b = Ephemeral::generate();
        let shared = a.shared(&b.public).unwrap();
        assert_eq!(shared, b.shared(&a.public).unwrap());
        assert_eq!(shared.len(), 32);

        let mut ours = derive(&shared, b"ctx");
        let mut theirs = derive(&shared, b"ctx");
        let first = ours.ours.seal(b"aad", b"one");
        let second = ours.ours.seal(b"aad", b"one");
        assert_ne!(first, second);
        assert_eq!(
            theirs.ours.open(b"aad", &first).as_deref(),
            Some(&b"one"[..])
        );
        assert!(theirs.ours.open(b"other", &second).is_none());
        assert_eq!(
            theirs.ours.open(b"aad", &second).as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(
            theirs
                .ours
                .open(b"aad", &ours.ours.seal(b"aad", b""))
                .as_deref(),
            Some(&[][..])
        );
    }

    #[test]
    fn the_nonce_carries_into_the_next_byte() {
        let mut ch = Channel {
            key: [0; 16],
            nonce: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff],
        };
        ch.step();
        assert_eq!(ch.nonce, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    }
}
