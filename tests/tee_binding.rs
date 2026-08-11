//! The report_data contract, pinned from ephemeris's side.
//!
//! These tests run WITHOUT the `tee` feature, because the property under test
//! is about what is hashed, not about talking to hardware.

use ed25519_dalek::pkcs8::EncodePublicKey;
use ephemeris::sign::{spki_der, SoftwareSigner};
use sha2::{Digest, Sha256};

fn key() -> ed25519_dalek::VerifyingKey {
    SoftwareSigner::generate([7u8; 32]).verifying_key()
}

#[test]
fn spki_der_is_the_whole_subject_public_key_info_not_the_key_bits() {
    // parallax's verifier hashes the entire SPKI DER — AlgorithmIdentifier and
    // the BIT STRING both. An attester that hashed the bare 32 key bytes
    // produces a digest that never matches, and finds out only on real
    // hardware against a real verifier.
    let k = key();
    let der = spki_der(&k).unwrap();
    let bare = k.to_bytes();

    assert_ne!(
        der.as_slice(),
        bare.as_slice(),
        "spki_der returned the bare key bits"
    );
    assert!(
        der.len() > bare.len(),
        "an SPKI wraps the key, so it is longer"
    );
    assert_ne!(
        Sha256::digest(&der).as_slice(),
        Sha256::digest(bare).as_slice(),
        "the key-bits digest must not equal the SPKI digest"
    );
}

#[test]
fn spki_der_matches_the_pkcs8_encoder() {
    // Our helper must not be a third encoding. It is the same DER
    // `EncodePublicKey` produces, which is what rustls, OpenSSL's i2d_PUBKEY
    // and rcgen's public_key_der all agree on.
    let k = key();
    let ours = spki_der(&k).unwrap();
    let theirs = k.to_public_key_der().unwrap();
    assert_eq!(ours.as_slice(), theirs.as_bytes());
}

#[test]
fn the_spki_changes_when_the_key_changes() {
    let a = spki_der(&SoftwareSigner::generate([1u8; 32]).verifying_key()).unwrap();
    let b = spki_der(&SoftwareSigner::generate([2u8; 32]).verifying_key()).unwrap();
    assert_ne!(a, b);
}
