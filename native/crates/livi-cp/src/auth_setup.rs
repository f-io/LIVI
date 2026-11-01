// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::future::Future;

use crate::crypto::{X25519, aes_ctr128, sha1, sha256};

pub trait MfiSigner {
    fn certificate(&self) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    fn sign(&self, digest: &[u8]) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    fn protocol_major(&self) -> impl Future<Output = Result<u8, String>> + Send;
}

const VERSION: u8 = 0x01;

pub async fn handle(body: &[u8], signer: &impl MfiSigner) -> Result<Option<Vec<u8>>, String> {
    if body.len() != 33 || body[0] != VERSION {
        println!(
            "[cp] auth-setup: bad request (len {}, version {:?})",
            body.len(),
            body.first()
        );
        return Ok(None);
    }
    let peer: [u8; 32] = body[1..].try_into().expect("checked length");
    let eph = X25519::generate();
    let ours = eph.public;
    let Some(shared) = eph.shared(&peer) else {
        println!("[cp] auth-setup: invalid shared secret");
        return Ok(None);
    };
    let aes_key: [u8; 16] = sha1(&[b"AES-KEY", &shared])[..16]
        .try_into()
        .expect("20 bytes");
    let aes_iv: [u8; 16] = sha1(&[b"AES-IV", &shared])[..16]
        .try_into()
        .expect("20 bytes");

    let cert = signer.certificate().await?;
    let digest = match signer.protocol_major().await? {
        2 => sha1(&[&ours, &peer]).to_vec(),
        _ => sha256(&[&ours, &peer]).to_vec(),
    };
    let sig = aes_ctr128(&aes_key, &aes_iv, &signer.sign(&digest).await?);
    println!(
        "[cp] auth-setup signed (cert {} B, sig {} B, digest {} B)",
        cert.len(),
        sig.len(),
        digest.len()
    );

    let mut out = ours.to_vec();
    out.extend_from_slice(&(cert.len() as u32).to_be_bytes());
    out.extend_from_slice(&cert);
    out.extend_from_slice(&(sig.len() as u32).to_be_bytes());
    out.extend_from_slice(&sig);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Chip {
        major: u8,
        digests: Mutex<Vec<Vec<u8>>>,
    }

    impl MfiSigner for Chip {
        async fn certificate(&self) -> Result<Vec<u8>, String> {
            Ok(b"CERT".to_vec())
        }
        async fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, String> {
            self.digests.lock().unwrap().push(digest.to_vec());
            Ok(vec![0x5a; 64])
        }
        async fn protocol_major(&self) -> Result<u8, String> {
            Ok(self.major)
        }
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("the test signer never waits"),
        }
    }

    fn request() -> (X25519, Vec<u8>) {
        let phone = X25519::generate();
        let body = [&[VERSION][..], &phone.public].concat();
        (phone, body)
    }

    #[test]
    fn a_3_0_chip_signs_sha256_and_the_phone_can_read_it() {
        let chip = Chip {
            major: 3,
            digests: Mutex::new(Vec::new()),
        };
        let (phone, body) = request();
        let phone_public = phone.public;
        let out = block_on(handle(&body, &chip)).unwrap().unwrap();

        let ours: [u8; 32] = out[..32].try_into().unwrap();
        assert_eq!(&out[32..36], &4u32.to_be_bytes());
        assert_eq!(&out[36..40], b"CERT");
        assert_eq!(&out[40..44], &64u32.to_be_bytes());
        assert_eq!(
            chip.digests.lock().unwrap()[0],
            sha256(&[&ours, &phone_public]).to_vec()
        );

        let shared = phone.shared(&ours).unwrap();
        let key: [u8; 16] = sha1(&[b"AES-KEY", &shared])[..16].try_into().unwrap();
        let iv: [u8; 16] = sha1(&[b"AES-IV", &shared])[..16].try_into().unwrap();
        assert_eq!(aes_ctr128(&key, &iv, &out[44..]), vec![0x5a; 64]);
    }

    #[test]
    fn a_2_0c_chip_signs_sha1() {
        let chip = Chip {
            major: 2,
            digests: Mutex::new(Vec::new()),
        };
        let (_, body) = request();
        block_on(handle(&body, &chip)).unwrap().unwrap();
        assert_eq!(chip.digests.lock().unwrap()[0].len(), 20);
    }

    #[test]
    fn malformed_requests_get_none() {
        let chip = Chip {
            major: 3,
            digests: Mutex::new(Vec::new()),
        };
        assert_eq!(block_on(handle(&[1; 32], &chip)), Ok(None));
        let (_, mut body) = request();
        body[0] = 2;
        assert_eq!(block_on(handle(&body, &chip)), Ok(None));
        assert_eq!(
            block_on(handle(&[&[VERSION][..], &[0; 32]].concat(), &chip)),
            Ok(None)
        );
        assert!(chip.digests.lock().unwrap().is_empty());
    }
}
