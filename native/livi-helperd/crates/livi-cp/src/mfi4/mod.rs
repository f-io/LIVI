//! Mutual authentication with a 3.0 coprocessor: the phone and LIVI prove each other over an
//! ephemeral P-256 exchange, then pair-setup runs through the channel it opens and the following
//! pair-verify is bound to it.

pub mod cert;
pub mod keys;
pub mod session;
pub mod wire;

use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::sign::Signer;

pub use session::Session;

use crate::crypto::hkdf_sha512;

/// The value of the X-Apple-AT header that asks for it.
pub const AUTH_TYPE: u32 = 0x20;

/// What binds a pair-verify to the session: the phone shows the first, we answer with the
/// second. Both are MACs over everything pair-verify exchanged.
pub fn transcript_proofs(shared: &[u8; 32], transcript: &[u8]) -> ([u8; 32], [u8; 32]) {
    let proof = |salt: &[u8], info: &[u8]| {
        let key = PKey::hmac(&hkdf_sha512(shared, salt, info)).expect("an HMAC key");
        let mut signer = Signer::new(MessageDigest::sha256(), &key).expect("HMAC-SHA256");
        signer.update(transcript).expect("HMAC update");
        signer.sign_to_vec().expect("HMAC").try_into().expect("32 bytes")
    };
    (proof(b"MFi4-KT1-Salt", b"MFi4-KT1-Info"), proof(b"MFi4-KT2-Salt", b"MFi4-KT2-Info"))
}
