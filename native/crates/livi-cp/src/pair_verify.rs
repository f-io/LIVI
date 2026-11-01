// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use crate::crypto::{
    X25519, chacha_open, chacha_seal, ed25519_sign, ed25519_verify, hkdf_sha512, nonce_label,
};
use crate::identity::Identity;
use crate::pair_setup::{error_reply, tlv};
use crate::pairings::Pairings;
use crate::tlv8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlKeys {
    pub read: [u8; 32],
    pub write: [u8; 32],
}

#[derive(Default)]
pub struct PairVerify {
    eph_public: Option<[u8; 32]>,
    client_eph_public: Option<[u8; 32]>,
    shared: Option<[u8; 32]>,
    enc_key: Option<[u8; 32]>,
    keys: Option<ControlKeys>,
    controller: Option<String>,
    transcript: Vec<u8>,
}

impl PairVerify {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn verified(&self) -> bool {
        self.keys.is_some()
    }

    pub fn controller_id(&self) -> Option<&str> {
        self.controller.as_deref()
    }

    pub fn control_keys(&self) -> Option<ControlKeys> {
        self.keys
    }

    pub fn shared_secret(&self) -> Option<[u8; 32]> {
        self.shared
    }

    pub fn transcript_proofs(&self) -> Option<([u8; 32], [u8; 32])> {
        self.keys.as_ref()?;
        Some(crate::mfi4::transcript_proofs(
            &self.shared?,
            &self.transcript,
        ))
    }

    pub fn handle(&mut self, body: &[u8], identity: &Identity, pairings: &Pairings) -> Vec<u8> {
        let msg = tlv8::decode(body);
        let state = msg.get(&tlv::STATE).and_then(|s| s.first().copied());
        if state == Some(1) {
            self.transcript.clear();
        }
        let reply = match state {
            Some(1) => self.m2(&msg, identity),
            Some(3) => self.m4(&msg, pairings),
            other => Err((other.unwrap_or(0), format!("unexpected state {other:?}"))),
        };
        match reply {
            Ok(out) => {
                self.transcript.extend_from_slice(body);
                self.transcript.extend_from_slice(&out);
                out
            }
            Err((reply_state, e)) => {
                println!("[cp] pair-verify: {e}");
                error_reply(reply_state)
            }
        }
    }

    fn m2(
        &mut self,
        msg: &BTreeMap<u8, Vec<u8>>,
        identity: &Identity,
    ) -> Result<Vec<u8>, (u8, String)> {
        let client = msg
            .get(&tlv::PUBLIC_KEY)
            .ok_or((2, "M1 without a public key".to_string()))?;
        let client: [u8; 32] = client
            .as_slice()
            .try_into()
            .map_err(|_| (1, "M1 key is no X25519 key".to_string()))?;
        let eph = X25519::generate();
        let eph_public = eph.public;
        let shared = eph
            .shared(&client)
            .ok_or((1, "M1 key forces a zero secret".to_string()))?;
        let enc_key = hkdf_sha512(
            &shared,
            b"Pair-Verify-Encrypt-Salt",
            b"Pair-Verify-Encrypt-Info",
        );

        let acc_id = identity.pairing_id.as_bytes();
        let sig = ed25519_sign(
            &identity.secret,
            &[eph_public.as_slice(), acc_id, &client].concat(),
        );
        let sub = tlv8::encode(&[(tlv::IDENTIFIER, acc_id), (tlv::SIGNATURE, &sig)]);
        let sealed = chacha_seal(&enc_key, &nonce_label("PV-Msg02"), &sub, &[]);

        self.eph_public = Some(eph_public);
        self.client_eph_public = Some(client);
        self.shared = Some(shared);
        self.enc_key = Some(enc_key);
        println!("[cp] pair-verify M1->M2");
        Ok(tlv8::encode(&[
            (tlv::STATE, &[2]),
            (tlv::PUBLIC_KEY, &eph_public),
            (tlv::ENCRYPTED_DATA, &sealed),
        ]))
    }

    fn m4(
        &mut self,
        msg: &BTreeMap<u8, Vec<u8>>,
        pairings: &Pairings,
    ) -> Result<Vec<u8>, (u8, String)> {
        let (Some(enc_key), Some(shared), Some(eph), Some(client), Some(sealed)) = (
            self.enc_key,
            self.shared,
            self.eph_public,
            self.client_eph_public,
            msg.get(&tlv::ENCRYPTED_DATA),
        ) else {
            return Err((4, "M3 before M1 or without data".into()));
        };
        let plain = chacha_open(&enc_key, &nonce_label("PV-Msg03"), sealed, &[])
            .ok_or((3, "M3 does not authenticate".to_string()))?;
        let sub = tlv8::decode(&plain);
        let (Some(ctrl_id), Some(ctrl_sig)) = (sub.get(&tlv::IDENTIFIER), sub.get(&tlv::SIGNATURE))
        else {
            return Err((4, "M3 lacks identifier or signature".into()));
        };
        let controller = String::from_utf8_lossy(ctrl_id).into_owned();
        let ltpk = pairings
            .get(&controller)
            .ok_or((4, format!("unknown controller {controller}")))?;
        let signed = [client.as_slice(), ctrl_id, &eph].concat();
        if !ed25519_verify(&ltpk, &signed, ctrl_sig) {
            return Err((4, "controller signature invalid".into()));
        }

        self.keys = Some(ControlKeys {
            read: hkdf_sha512(&shared, b"Control-Salt", b"Control-Write-Encryption-Key"),
            write: hkdf_sha512(&shared, b"Control-Salt", b"Control-Read-Encryption-Key"),
        });
        println!("[cp] pair-verify M3->M4, verified {controller}");
        self.controller = Some(controller);
        Ok(tlv8::encode(&[(tlv::STATE, &[4])]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{ed25519_public, random_bytes};
    use crate::identity::load_or_create;
    use crate::pair_setup::ERR_AUTHENTICATION;
    use crate::pairings::tests::TempDir;

    struct Phone {
        secret: [u8; 32],
        id: &'static [u8],
    }

    fn state_and_error(reply: &[u8]) -> (u8, Option<u8>) {
        let m = tlv8::decode(reply);
        (m[&tlv::STATE][0], m.get(&tlv::ERROR).map(|e| e[0]))
    }

    fn run(
        verify: &mut PairVerify,
        identity: &Identity,
        pairings: &Pairings,
        phone: &Phone,
    ) -> (Vec<u8>, [u8; 32]) {
        let eph = X25519::generate();
        let eph_public = eph.public;
        let m2 = tlv8::decode(&verify.handle(
            &tlv8::encode(&[(tlv::STATE, &[1]), (tlv::PUBLIC_KEY, &eph_public)]),
            identity,
            pairings,
        ));
        let acc_eph: [u8; 32] = m2[&tlv::PUBLIC_KEY].as_slice().try_into().unwrap();
        let shared = eph.shared(&acc_eph).unwrap();
        let enc_key = hkdf_sha512(
            &shared,
            b"Pair-Verify-Encrypt-Salt",
            b"Pair-Verify-Encrypt-Info",
        );

        let sub = tlv8::decode(
            &chacha_open(
                &enc_key,
                &nonce_label("PV-Msg02"),
                &m2[&tlv::ENCRYPTED_DATA],
                &[],
            )
            .unwrap(),
        );
        let signed = [
            acc_eph.as_slice(),
            identity.pairing_id.as_bytes(),
            &eph_public,
        ]
        .concat();
        assert!(ed25519_verify(
            &identity.public,
            &signed,
            &sub[&tlv::SIGNATURE]
        ));

        let sig = ed25519_sign(
            &phone.secret,
            &[eph_public.as_slice(), phone.id, &acc_eph].concat(),
        );
        let sub = tlv8::encode(&[(tlv::IDENTIFIER, phone.id), (tlv::SIGNATURE, &sig)]);
        let sealed = chacha_seal(&enc_key, &nonce_label("PV-Msg03"), &sub, &[]);
        let m4 = verify.handle(
            &tlv8::encode(&[(tlv::STATE, &[3]), (tlv::ENCRYPTED_DATA, &sealed)]),
            identity,
            pairings,
        );
        (m4, shared)
    }

    #[test]
    fn a_paired_phone_verifies_and_gets_crossed_keys() {
        let dir = TempDir::new();
        let identity = load_or_create(&dir.0.join("identity.json"));
        let pairings = Pairings::new(dir.0.join("pairings.json"));
        let phone = Phone {
            secret: random_bytes(),
            id: b"phone-1",
        };
        pairings.save("phone-1", &ed25519_public(&phone.secret));

        let mut verify = PairVerify::new();
        let (m4, shared) = run(&mut verify, &identity, &pairings, &phone);
        assert_eq!(state_and_error(&m4), (4, None));
        assert!(verify.verified());
        assert_eq!(verify.controller_id(), Some("phone-1"));
        assert_eq!(verify.shared_secret(), Some(shared));
        let keys = verify.control_keys().unwrap();
        assert_eq!(
            keys.read,
            hkdf_sha512(&shared, b"Control-Salt", b"Control-Write-Encryption-Key")
        );
        assert_eq!(
            keys.write,
            hkdf_sha512(&shared, b"Control-Salt", b"Control-Read-Encryption-Key")
        );
    }

    #[test]
    fn an_unknown_or_forged_phone_is_refused() {
        let dir = TempDir::new();
        let identity = load_or_create(&dir.0.join("identity.json"));
        let pairings = Pairings::new(dir.0.join("pairings.json"));
        let stranger = Phone {
            secret: random_bytes(),
            id: b"stranger",
        };
        let (m4, _) = run(&mut PairVerify::new(), &identity, &pairings, &stranger);
        assert_eq!(state_and_error(&m4), (4, Some(ERR_AUTHENTICATION)));

        let impostor = Phone {
            secret: random_bytes(),
            id: b"phone-1",
        };
        pairings.save("phone-1", &ed25519_public(&random_bytes()));
        let mut verify = PairVerify::new();
        let (m4, _) = run(&mut verify, &identity, &pairings, &impostor);
        assert_eq!(state_and_error(&m4), (4, Some(ERR_AUTHENTICATION)));
        assert!(!verify.verified());
    }

    #[test]
    fn broken_messages_answer_errors() {
        let dir = TempDir::new();
        let identity = load_or_create(&dir.0.join("identity.json"));
        let pairings = Pairings::new(dir.0.join("pairings.json"));
        let mut verify = PairVerify::new();
        let mut h = |b: &[u8]| state_and_error(&verify.handle(b, &identity, &pairings));
        assert_eq!(h(&[]), (0, Some(ERR_AUTHENTICATION)));
        assert_eq!(
            h(&tlv8::encode(&[(tlv::STATE, &[5])])),
            (5, Some(ERR_AUTHENTICATION))
        );
        assert_eq!(
            h(&tlv8::encode(&[(tlv::STATE, &[1])])),
            (2, Some(ERR_AUTHENTICATION))
        );
        assert_eq!(
            h(&tlv8::encode(&[
                (tlv::STATE, &[1]),
                (tlv::PUBLIC_KEY, &[0; 32])
            ])),
            (1, Some(ERR_AUTHENTICATION))
        );
        assert_eq!(
            h(&tlv8::encode(&[(tlv::STATE, &[3])])),
            (4, Some(ERR_AUTHENTICATION))
        );
        let m1 = tlv8::encode(&[
            (tlv::STATE, &[1]),
            (tlv::PUBLIC_KEY, &X25519::generate().public),
        ]);
        assert_eq!(h(&m1).0, 2);
        let junk = tlv8::encode(&[(tlv::STATE, &[3]), (tlv::ENCRYPTED_DATA, &[1, 2, 3])]);
        assert_eq!(h(&junk), (3, Some(ERR_AUTHENTICATION)));
    }
}
