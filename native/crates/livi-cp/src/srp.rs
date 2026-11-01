// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::OnceLock;

use num_bigint::BigUint;

use crate::crypto::{random_bytes, sha512};

const N_HEX: &str = "\
FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74\
020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437\
4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05\
98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB\
9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B\
E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183\
995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507A33A\
85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7AB\
F5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864D8\
7602733EC86A64521F2B18177B200CBBE117577A615D6C770988C0BAD946E208\
E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF";
const N_BYTES: usize = 384;

pub(crate) fn n() -> &'static BigUint {
    static N: OnceLock<BigUint> = OnceLock::new();
    N.get_or_init(|| {
        BigUint::parse_bytes(N_HEX.as_bytes(), 16).expect("the group prime is valid hex")
    })
}

pub(crate) fn g() -> BigUint {
    BigUint::from(5u8)
}

pub(crate) fn to_bytes(n: &BigUint) -> Vec<u8> {
    n.to_bytes_be()
}

pub(crate) fn pad(n: &BigUint) -> Vec<u8> {
    let b = to_bytes(n);
    if b.len() >= N_BYTES {
        return b;
    }
    [vec![0u8; N_BYTES - b.len()], b].concat()
}

pub(crate) fn h(parts: &[&[u8]]) -> BigUint {
    BigUint::from_bytes_be(&sha512(parts))
}

fn k() -> &'static BigUint {
    static K: OnceLock<BigUint> = OnceLock::new();
    K.get_or_init(|| h(&[&to_bytes(n()), &pad(&g())]))
}

pub struct Verified {
    pub session_key: [u8; 64],
    pub server_proof: [u8; 64],
}

pub struct SrpServer {
    pub salt: [u8; 16],
    pub public: Vec<u8>,
    identity: Vec<u8>,
    v: BigUint,
    b: BigUint,
    big_b: BigUint,
}

impl SrpServer {
    pub fn start(username: &str, password: &str) -> Self {
        Self::with_secrets(username, password, random_bytes(), random_bytes::<32>())
    }

    pub(crate) fn with_secrets(
        username: &str,
        password: &str,
        salt: [u8; 16],
        b: [u8; 32],
    ) -> Self {
        let identity = username.as_bytes().to_vec();
        let inner = sha512(&[&identity, b":", password.as_bytes()]);
        let x = h(&[&salt, &inner]);
        let v = g().modpow(&x, n());
        let b = BigUint::from_bytes_be(&b);
        let big_b = (k() * &v + g().modpow(&b, n())) % n();
        Self {
            salt,
            public: pad(&big_b),
            identity,
            v,
            b,
            big_b,
        }
    }

    pub fn verify(&self, a: &[u8], client_proof: &[u8]) -> Option<Verified> {
        let a = BigUint::from_bytes_be(a);
        if (&a % n()) == BigUint::ZERO {
            return None;
        }
        let u = h(&[&pad(&a), &pad(&self.big_b)]);
        let s = ((&a * self.v.modpow(&u, n())) % n()).modpow(&self.b, n());
        let session_key = sha512(&[&to_bytes(&s)]);

        let hn = sha512(&[&to_bytes(n())]);
        let hg = sha512(&[&to_bytes(&g())]);
        let xor: Vec<u8> = hn.iter().zip(hg.iter()).map(|(a, b)| a ^ b).collect();
        let expected = sha512(&[
            &xor,
            &sha512(&[&self.identity]),
            &self.salt,
            &pad(&a),
            &self.public,
            &session_key,
        ]);
        if expected.as_slice() != client_proof {
            return None;
        }
        let server_proof = sha512(&[&pad(&a), client_proof, &session_key]);
        Some(Verified {
            session_key,
            server_proof,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn client(
        server: &SrpServer,
        username: &str,
        password: &str,
    ) -> (Vec<u8>, [u8; 64], [u8; 64]) {
        let a = BigUint::from_bytes_be(&[0x42; 32]);
        let big_a = g().modpow(&a, n());
        let big_b = BigUint::from_bytes_be(&server.public);
        let u = h(&[&pad(&big_a), &pad(&big_b)]);
        let x = h(&[
            &server.salt,
            &sha512(&[username.as_bytes(), b":", password.as_bytes()]),
        ]);
        let gx = g().modpow(&x, n());
        let base = (&big_b + n() * k() - (k() * gx) % n()) % n();
        let s = base.modpow(&(&a + &u * &x), n());
        let key = sha512(&[&to_bytes(&s)]);
        let hn = sha512(&[&to_bytes(n())]);
        let hg = sha512(&[&to_bytes(&g())]);
        let xor: Vec<u8> = hn.iter().zip(hg.iter()).map(|(a, b)| a ^ b).collect();
        let m1 = sha512(&[
            &xor,
            &sha512(&[username.as_bytes()]),
            &server.salt,
            &pad(&big_a),
            &pad(&big_b),
            &key,
        ]);
        (pad(&big_a), m1, key)
    }

    #[test]
    fn a_client_with_the_code_verifies() {
        let server = SrpServer::start("Pair-Setup", "3939");
        assert_eq!(server.public.len(), N_BYTES);
        let (a, m1, key) = client(&server, "Pair-Setup", "3939");
        let ok = server.verify(&a, &m1).unwrap();
        assert_eq!(ok.session_key, key);
        assert_eq!(ok.server_proof, sha512(&[&a, &m1, &key]));
    }

    #[test]
    fn a_wrong_code_or_a_zero_key_fails() {
        let server = SrpServer::start("Pair-Setup", "3939");
        let (a, m1, _) = client(&server, "Pair-Setup", "0000");
        assert!(server.verify(&a, &m1).is_none());
        assert!(server.verify(&to_bytes(n()), &m1).is_none());
        assert!(server.verify(&[0], &m1).is_none());
    }

    #[test]
    fn small_values_pad_and_zero_is_one_byte() {
        assert_eq!(pad(&BigUint::from(5u8)).len(), N_BYTES);
        assert_eq!(to_bytes(&BigUint::ZERO), vec![0]);
    }
}
