// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey as _;
use rsa::pkcs8::DecodePrivateKey as _;
use rsa::{Pkcs1v15Sign, RsaPrivateKey};

use crate::{AuthCoprocessor, CHALLENGE_MAX, CHALLENGE_MIN, MfiError};

pub const CERTIFICATE_FILE: &str = "certificate.p7b";
pub const IDENTITY_FILE: &str = "identity.pk8";

const SHA1_LEN: usize = 20;
const SHA1_PREFIX: [u8; 15] = [
    0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
];

enum Identity {
    Ec(SigningKey),
    Rsa(RsaPrivateKey),
}

pub struct FileCoprocessor {
    certificate: Vec<u8>,
    identity: Identity,
}

fn bad(what: impl Into<String>) -> MfiError {
    MfiError::File(what.into())
}

pub fn der(bytes: &[u8]) -> Result<Vec<u8>, MfiError> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(bytes.to_vec());
    };
    if !text.trim_start().starts_with("-----BEGIN") {
        return Ok(bytes.to_vec());
    }
    let body: String = text
        .lines()
        .map(str::trim)
        .skip_while(|l| !l.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END"))
        .collect();
    STANDARD
        .decode(body)
        .map_err(|e| bad(format!("PEM body is not base64: {e}")))
}

pub fn certificate(bytes: &[u8]) -> Result<Vec<u8>, MfiError> {
    let der = der(bytes)?;
    if der.first() != Some(&0x30) || der.len() < 16 {
        return Err(bad("the certificate is not a DER or PEM PKCS#7 file"));
    }
    Ok(der)
}

fn identity(bytes: &[u8]) -> Result<Identity, MfiError> {
    let der = der(bytes)?;
    if let Ok(key) = SigningKey::from_pkcs8_der(&der) {
        return Ok(Identity::Ec(key));
    }
    RsaPrivateKey::from_pkcs8_der(&der)
        .map(Identity::Rsa)
        .map_err(|_| bad("the identity is not an unencrypted PKCS#8 P-256 or RSA key"))
}

fn major(identity: &Identity) -> u8 {
    match identity {
        Identity::Ec(_) => 3,
        Identity::Rsa(_) => 2,
    }
}

pub fn identity_major(bytes: &[u8]) -> Result<u8, MfiError> {
    identity(bytes).map(|i| major(&i))
}

impl FileCoprocessor {
    pub fn from_bytes(certificate_bytes: &[u8], identity_bytes: &[u8]) -> Result<Self, MfiError> {
        Ok(Self {
            certificate: certificate(certificate_bytes)?,
            identity: identity(identity_bytes)?,
        })
    }

    pub fn open(dir: &Path) -> Result<Self, MfiError> {
        let read =
            |name: &str| std::fs::read(dir.join(name)).map_err(|e| bad(format!("{name}: {e}")));
        Self::from_bytes(&read(CERTIFICATE_FILE)?, &read(IDENTITY_FILE)?)
    }
}

impl AuthCoprocessor for FileCoprocessor {
    fn protocol_major(&mut self) -> Result<u8, MfiError> {
        Ok(major(&self.identity))
    }

    fn read_certificate(&mut self) -> Result<Vec<u8>, MfiError> {
        Ok(self.certificate.clone())
    }

    fn generate_challenge_response(&mut self, challenge: &[u8]) -> Result<Vec<u8>, MfiError> {
        if !(CHALLENGE_MIN..=CHALLENGE_MAX).contains(&challenge.len()) {
            return Err(MfiError::ChallengeSize(challenge.len()));
        }
        match &self.identity {
            Identity::Ec(key) => {
                let signature: Signature = key
                    .sign_prehash(challenge)
                    .map_err(|e| bad(format!("ECDSA signing failed: {e}")))?;
                Ok(signature.to_bytes().to_vec())
            }
            Identity::Rsa(key) => {
                if challenge.len() != SHA1_LEN {
                    return Err(MfiError::ChallengeSize(challenge.len()));
                }
                let scheme = Pkcs1v15Sign {
                    hash_len: Some(SHA1_LEN),
                    prefix: SHA1_PREFIX.into(),
                };
                key.sign(scheme, challenge)
                    .map_err(|e| bad(format!("RSA signing failed: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    use rsa::RsaPublicKey;

    use super::*;

    const EC_PEM: &str = include_str!("testdata/ec-p256.pk8.pem");
    const EC_DER: &[u8] = include_bytes!("testdata/ec-p256.pk8");
    const RSA_PEM: &str = include_str!("testdata/rsa-1024.pk8.pem");
    const CERT_PEM: &str = include_str!("testdata/certificate.p7b.pem");
    const CERT_DER: &[u8] = include_bytes!("testdata/certificate.p7b");

    #[test]
    fn pem_and_der_give_the_same_certificate() {
        assert_eq!(certificate(CERT_PEM.as_bytes()).unwrap(), CERT_DER);
        assert_eq!(certificate(CERT_DER).unwrap(), CERT_DER);
        assert!(certificate(b"hello").is_err());
    }

    #[test]
    fn the_key_type_picks_the_generation() {
        assert_eq!(identity_major(EC_PEM.as_bytes()).unwrap(), 3);
        assert_eq!(identity_major(EC_DER).unwrap(), 3);
        assert_eq!(identity_major(RSA_PEM.as_bytes()).unwrap(), 2);
        assert!(identity_major(CERT_DER).is_err());
    }

    #[test]
    fn a_p256_identity_signs_the_digest_as_raw_r_and_s() {
        let mut chip = FileCoprocessor::from_bytes(CERT_DER, EC_PEM.as_bytes()).unwrap();
        assert_eq!(chip.protocol_major().unwrap(), 3);
        assert_eq!(chip.read_certificate().unwrap(), CERT_DER);
        let digest = [7u8; 32];
        let raw = chip.generate_challenge_response(&digest).unwrap();
        assert_eq!(raw.len(), 64);
        let key = SigningKey::from_pkcs8_der(EC_DER).unwrap();
        let signature = Signature::from_slice(&raw).unwrap();
        VerifyingKey::from(&key)
            .verify_prehash(&digest, &signature)
            .unwrap();
    }

    #[test]
    fn an_rsa_identity_signs_a_sha1_digest() {
        let mut chip = FileCoprocessor::from_bytes(CERT_DER, RSA_PEM.as_bytes()).unwrap();
        assert_eq!(chip.protocol_major().unwrap(), 2);
        let digest = [9u8; SHA1_LEN];
        let signature = chip.generate_challenge_response(&digest).unwrap();
        assert_eq!(signature.len(), 128);
        let key = RsaPrivateKey::from_pkcs8_der(&der(RSA_PEM.as_bytes()).unwrap()).unwrap();
        let scheme = Pkcs1v15Sign {
            hash_len: Some(SHA1_LEN),
            prefix: SHA1_PREFIX.into(),
        };
        RsaPublicKey::from(&key)
            .verify(scheme, &digest, &signature)
            .unwrap();
        assert!(chip.generate_challenge_response(&[1u8; 32]).is_err());
    }

    #[test]
    fn the_files_load_from_their_folder() {
        let dir = std::env::temp_dir().join(format!("livi-mfi-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(FileCoprocessor::open(&dir).is_err());
        std::fs::write(dir.join(CERTIFICATE_FILE), CERT_PEM).unwrap();
        std::fs::write(dir.join(IDENTITY_FILE), EC_DER).unwrap();
        let mut chip = FileCoprocessor::open(&dir).unwrap();
        assert_eq!(chip.read_certificate().unwrap(), CERT_DER);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
