// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use openssl::bn::BigNumContext;
use openssl::cipher::Cipher as Aead;
use openssl::cipher_ctx::CipherCtx;
use openssl::derive::Deriver;
use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
use openssl::error::ErrorStack;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::sign::Signer;
use openssl::symm::Cipher;

use crate::crypto::sha256;

pub const POINT: usize = 33;
pub const TAG: usize = 16;
const EXPANDED: usize = 200;
const EXTRACTION: &[u8] = b"MFi 4.0 SIGMA-I Authentication Randomness Extraction";
const EXPANSION: &[u8] = b"MFi 4.0 SIGMA-I Authentication Key Expansion";
pub const SIGMA: u8 = 0x01;

pub fn group() -> EcGroup {
    EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).expect("P-256 is built in")
}

pub struct Ephemeral {
    key: EcKey<Private>,
    pub public: [u8; POINT],
}

impl Ephemeral {
    pub fn generate() -> Result<Self, String> {
        let group = group();
        let key = EcKey::generate(&group).map_err(|e| e.to_string())?;
        let mut ctx = BigNumContext::new().map_err(|e| e.to_string())?;
        let public = key
            .public_key()
            .to_bytes(&group, PointConversionForm::COMPRESSED, &mut ctx)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            key,
            public: public
                .try_into()
                .map_err(|_| "a point that is no 33 bytes")?,
        })
    }

    pub fn shared(&self, peer: &[u8; POINT]) -> Result<Vec<u8>, String> {
        let group = group();
        let mut ctx = BigNumContext::new().map_err(|e| e.to_string())?;
        let point = EcPoint::from_bytes(&group, peer, &mut ctx).map_err(|e| e.to_string())?;
        let peer = EcKey::from_public_key(&group, &point).map_err(|e| e.to_string())?;
        peer.check_key().map_err(|e| e.to_string())?;
        let ours = PKey::from_ec_key(self.key.clone()).map_err(|e| e.to_string())?;
        let theirs = PKey::from_ec_key(peer).map_err(|e| e.to_string())?;
        let mut deriver = Deriver::new(&ours).map_err(|e| e.to_string())?;
        deriver.set_peer(&theirs).map_err(|e| e.to_string())?;
        deriver.derive_to_vec().map_err(|e| e.to_string())
    }
}

pub fn cmac(key: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let pkey = PKey::cmac(&Cipher::aes_128_cbc(), key).expect("a 16 byte AES key");
    let mut signer = Signer::new_without_digest(&pkey).expect("CMAC signer");
    signer.update(data).expect("CMAC update");
    signer
        .sign_to_vec()
        .expect("CMAC")
        .try_into()
        .expect("one AES block")
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
        let out = ccm(&self.key, &self.nonce, aad, plain, None)
            .expect("CCM with a 16 byte key and a 12 byte nonce");
        self.step();
        out
    }

    pub fn open(&mut self, aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        let (data, tag) = sealed.split_at_checked(sealed.len().checked_sub(TAG)?)?;
        let plain = ccm(&self.key, &self.nonce, aad, data, Some(tag)).ok()?;
        self.step();
        Some(plain)
    }
}

fn ccm(
    key: &[u8; 16],
    nonce: &[u8; 12],
    aad: &[u8],
    data: &[u8],
    tag: Option<&[u8]>,
) -> Result<Vec<u8>, ErrorStack> {
    let cipher = Aead::aes_128_ccm();
    let mut ctx = CipherCtx::new()?;
    match tag {
        None => ctx.encrypt_init(Some(cipher), None, None)?,
        Some(_) => ctx.decrypt_init(Some(cipher), None, None)?,
    }
    ctx.set_iv_length(nonce.len())?;
    match tag {
        None => ctx.set_tag_length(TAG)?,
        Some(tag) => ctx.set_tag(tag)?,
    }
    match tag {
        None => ctx.encrypt_init(None, Some(key), Some(nonce))?,
        Some(_) => ctx.decrypt_init(None, Some(key), Some(nonce))?,
    }
    ctx.set_data_len(data.len())?;
    if !aad.is_empty() {
        ctx.cipher_update(aad, None)?;
    }
    let mut out = vec![0; data.len() + cipher.block_size()];
    let mut n = ctx.cipher_update(data, Some(&mut out))?;
    if tag.is_none() {
        n += ctx.cipher_final(&mut out[n..])?;
        out.truncate(n);
        let mut tag = [0; TAG];
        ctx.tag(&mut tag)?;
        out.extend_from_slice(&tag);
    } else {
        out.truncate(n);
    }
    Ok(out)
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

    #[test]
    fn both_ends_agree_and_a_channel_counts_its_nonce() {
        let a = Ephemeral::generate().unwrap();
        let b = Ephemeral::generate().unwrap();
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
