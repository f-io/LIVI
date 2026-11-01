// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::crypto::{
    chacha_open, chacha_seal, ed25519_sign, ed25519_verify, hkdf_sha512, nonce_label,
};
use crate::identity::Identity;
use crate::pairings::Pairings;
use crate::srp::SrpServer;
use crate::tlv8;

pub(crate) mod tlv {
    pub const IDENTIFIER: u8 = 0x01;
    pub const SALT: u8 = 0x02;
    pub const PUBLIC_KEY: u8 = 0x03;
    pub const PROOF: u8 = 0x04;
    pub const ENCRYPTED_DATA: u8 = 0x05;
    pub const STATE: u8 = 0x06;
    pub const ERROR: u8 = 0x07;
    pub const SIGNATURE: u8 = 0x0a;
}

const SETUP_CODE: &str = "3939";
pub(crate) const ERR_AUTHENTICATION: u8 = 2;

pub(crate) fn error_reply(state: u8) -> Vec<u8> {
    tlv8::encode(&[(tlv::STATE, &[state]), (tlv::ERROR, &[ERR_AUTHENTICATION])])
}

#[derive(Default)]
pub struct PairSetup {
    srp: Option<SrpServer>,
    session_key: Option<[u8; 64]>,
    complete: bool,
}

impl PairSetup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn complete(&self) -> bool {
        self.complete
    }

    pub fn handle(&mut self, body: &[u8], identity: &Identity, pairings: &Pairings) -> Vec<u8> {
        let msg = tlv8::decode(body);
        let state = msg.get(&tlv::STATE).and_then(|s| s.first().copied());
        let reply = match state {
            Some(1) => Ok(self.m2()),
            Some(3) => self.m4(&msg).map_err(|e| (4, e)),
            Some(5) => self.m6(&msg, identity, pairings),
            other => Err((other.unwrap_or(0), format!("unexpected state {other:?}"))),
        };
        reply.unwrap_or_else(|(reply_state, e)| {
            println!("[cp] pair-setup: {e}");
            error_reply(reply_state)
        })
    }

    fn m2(&mut self) -> Vec<u8> {
        let srp = SrpServer::start("Pair-Setup", SETUP_CODE);
        let reply = tlv8::encode(&[
            (tlv::STATE, &[2]),
            (tlv::PUBLIC_KEY, &srp.public),
            (tlv::SALT, &srp.salt),
        ]);
        self.srp = Some(srp);
        println!("[cp] pair-setup M1->M2");
        reply
    }

    fn m4(&mut self, msg: &std::collections::BTreeMap<u8, Vec<u8>>) -> Result<Vec<u8>, String> {
        let (Some(srp), Some(a), Some(proof)) =
            (&self.srp, msg.get(&tlv::PUBLIC_KEY), msg.get(&tlv::PROOF))
        else {
            return Err("M3 before M1 or without key and proof".into());
        };
        let verified = srp.verify(a, proof).ok_or("SRP proof does not match")?;
        self.session_key = Some(verified.session_key);
        println!("[cp] pair-setup M3->M4");
        Ok(tlv8::encode(&[
            (tlv::STATE, &[4]),
            (tlv::PROOF, &verified.server_proof),
        ]))
    }

    fn m6(
        &mut self,
        msg: &std::collections::BTreeMap<u8, Vec<u8>>,
        identity: &Identity,
        pairings: &Pairings,
    ) -> Result<Vec<u8>, (u8, String)> {
        let (Some(k), Some(sealed)) = (&self.session_key, msg.get(&tlv::ENCRYPTED_DATA)) else {
            return Err((6, "M5 before the SRP exchange or without data".into()));
        };
        let key = hkdf_sha512(k, b"Pair-Setup-Encrypt-Salt", b"Pair-Setup-Encrypt-Info");
        let plain = chacha_open(&key, &nonce_label("PS-Msg05"), sealed, &[])
            .ok_or((5, "M5 does not authenticate".to_string()))?;
        let sub = tlv8::decode(&plain);
        let (Some(ctrl_id), Some(ctrl_ltpk), Some(ctrl_sig)) = (
            sub.get(&tlv::IDENTIFIER),
            sub.get(&tlv::PUBLIC_KEY),
            sub.get(&tlv::SIGNATURE),
        ) else {
            return Err((6, "M5 lacks identifier, key or signature".into()));
        };

        let ctrl_sign_key = hkdf_sha512(
            k,
            b"Pair-Setup-Controller-Sign-Salt",
            b"Pair-Setup-Controller-Sign-Info",
        );
        let signed = [ctrl_sign_key.as_slice(), ctrl_id, ctrl_ltpk].concat();
        if !ed25519_verify(ctrl_ltpk, &signed, ctrl_sig) {
            return Err((6, "controller signature invalid".into()));
        }
        let controller = String::from_utf8_lossy(ctrl_id).into_owned();
        pairings.save(&controller, ctrl_ltpk);

        let acc_id = identity.pairing_id.as_bytes();
        let acc_sign_key = hkdf_sha512(
            k,
            b"Pair-Setup-Accessory-Sign-Salt",
            b"Pair-Setup-Accessory-Sign-Info",
        );
        let acc_sig = ed25519_sign(
            &identity.secret,
            &[acc_sign_key.as_slice(), acc_id, &identity.public].concat(),
        );
        let sub = tlv8::encode(&[
            (tlv::IDENTIFIER, acc_id),
            (tlv::PUBLIC_KEY, &identity.public),
            (tlv::SIGNATURE, &acc_sig),
        ]);
        let sealed = chacha_seal(&key, &nonce_label("PS-Msg06"), &sub, &[]);
        self.complete = true;
        println!("[cp] pair-setup M5->M6, paired with {controller}");
        Ok(tlv8::encode(&[
            (tlv::STATE, &[6]),
            (tlv::ENCRYPTED_DATA, &sealed),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{ed25519_public, random_bytes};
    use crate::identity::load_or_create;
    use crate::pairings::tests::TempDir;
    use crate::srp::tests::client;

    struct Env {
        _dir: TempDir,
        identity: Identity,
        pairings: Pairings,
    }

    fn env() -> Env {
        let dir = TempDir::new();
        let identity = load_or_create(&dir.0.join("identity.json"));
        let pairings = Pairings::new(dir.0.join("pairings.json"));
        Env {
            _dir: dir,
            identity,
            pairings,
        }
    }

    fn state_and_error(reply: &[u8]) -> (u8, Option<u8>) {
        let m = tlv8::decode(reply);
        (m[&tlv::STATE][0], m.get(&tlv::ERROR).map(|e| e[0]))
    }

    fn through_m4(setup: &mut PairSetup, env: &Env, code: &str) -> Option<[u8; 64]> {
        let m2 = tlv8::decode(&setup.handle(
            &tlv8::encode(&[(tlv::STATE, &[1])]),
            &env.identity,
            &env.pairings,
        ));
        let srp = setup.srp.as_ref().unwrap();
        assert_eq!(m2[&tlv::PUBLIC_KEY], srp.public);
        let (a, m1, key) = client(srp, "Pair-Setup", code);
        let m3 = tlv8::encode(&[(tlv::STATE, &[3]), (tlv::PUBLIC_KEY, &a), (tlv::PROOF, &m1)]);
        let m4 = tlv8::decode(&setup.handle(&m3, &env.identity, &env.pairings));
        m4.get(&tlv::PROOF).map(|_| key)
    }

    fn m5(key: &[u8; 64], ctrl_secret: &[u8; 32], ctrl_id: &[u8], bad_sig: bool) -> Vec<u8> {
        let ltpk = ed25519_public(ctrl_secret);
        let sign_key = hkdf_sha512(
            key,
            b"Pair-Setup-Controller-Sign-Salt",
            b"Pair-Setup-Controller-Sign-Info",
        );
        let mut sig = ed25519_sign(ctrl_secret, &[sign_key.as_slice(), ctrl_id, &ltpk].concat());
        if bad_sig {
            sig[0] ^= 1;
        }
        let sub = tlv8::encode(&[
            (tlv::IDENTIFIER, ctrl_id),
            (tlv::PUBLIC_KEY, &ltpk),
            (tlv::SIGNATURE, &sig),
        ]);
        let enc_key = hkdf_sha512(key, b"Pair-Setup-Encrypt-Salt", b"Pair-Setup-Encrypt-Info");
        let sealed = chacha_seal(&enc_key, &nonce_label("PS-Msg05"), &sub, &[]);
        tlv8::encode(&[(tlv::STATE, &[5]), (tlv::ENCRYPTED_DATA, &sealed)])
    }

    #[test]
    fn a_phone_pairs_and_both_long_term_keys_check_out() {
        let env = env();
        let mut setup = PairSetup::new();
        let key = through_m4(&mut setup, &env, "3939").unwrap();
        let ctrl_secret = random_bytes::<32>();
        let reply = setup.handle(
            &m5(&key, &ctrl_secret, b"phone-1", false),
            &env.identity,
            &env.pairings,
        );
        assert!(setup.complete());
        assert_eq!(
            env.pairings.get("phone-1"),
            Some(ed25519_public(&ctrl_secret).to_vec())
        );

        let m6 = tlv8::decode(&reply);
        assert_eq!(m6[&tlv::STATE], vec![6]);
        let enc_key = hkdf_sha512(&key, b"Pair-Setup-Encrypt-Salt", b"Pair-Setup-Encrypt-Info");
        let sub = tlv8::decode(
            &chacha_open(
                &enc_key,
                &nonce_label("PS-Msg06"),
                &m6[&tlv::ENCRYPTED_DATA],
                &[],
            )
            .unwrap(),
        );
        assert_eq!(sub[&tlv::IDENTIFIER], env.identity.pairing_id.as_bytes());
        assert_eq!(sub[&tlv::PUBLIC_KEY], env.identity.public);
        let sign_key = hkdf_sha512(
            &key,
            b"Pair-Setup-Accessory-Sign-Salt",
            b"Pair-Setup-Accessory-Sign-Info",
        );
        let signed = [
            sign_key.as_slice(),
            env.identity.pairing_id.as_bytes(),
            &env.identity.public,
        ]
        .concat();
        assert!(ed25519_verify(
            &env.identity.public,
            &signed,
            &sub[&tlv::SIGNATURE]
        ));
    }

    #[test]
    fn a_wrong_code_ends_at_m4() {
        let env = env();
        let mut setup = PairSetup::new();
        assert_eq!(through_m4(&mut setup, &env, "1234"), None);
    }

    #[test]
    fn out_of_order_and_broken_messages_answer_errors() {
        let env = env();
        let mut setup = PairSetup::new();
        let h = |s: &mut PairSetup, b: &[u8]| {
            state_and_error(&s.handle(b, &env.identity, &env.pairings))
        };
        assert_eq!(h(&mut setup, &[]), (0, Some(ERR_AUTHENTICATION)));
        assert_eq!(
            h(&mut setup, &tlv8::encode(&[(tlv::STATE, &[7])])),
            (7, Some(ERR_AUTHENTICATION))
        );
        assert_eq!(
            h(&mut setup, &tlv8::encode(&[(tlv::STATE, &[3])])),
            (4, Some(ERR_AUTHENTICATION))
        );
        assert_eq!(
            h(&mut setup, &tlv8::encode(&[(tlv::STATE, &[5])])),
            (6, Some(ERR_AUTHENTICATION))
        );

        let key = through_m4(&mut setup, &env, "3939").unwrap();
        let bad = m5(&key, &random_bytes::<32>(), b"phone-2", true);
        assert_eq!(h(&mut setup, &bad), (6, Some(ERR_AUTHENTICATION)));
        assert_eq!(env.pairings.get("phone-2"), None);
        let garbage = tlv8::encode(&[(tlv::STATE, &[5]), (tlv::ENCRYPTED_DATA, &[1, 2, 3])]);
        assert_eq!(h(&mut setup, &garbage), (5, Some(ERR_AUTHENTICATION)));
        assert!(!setup.complete());
    }
}
