use ephemeris::sign::{Custody, Signer, SoftwareSigner};

#[test]
fn a_software_signer_declares_software_custody() {
    let s = SoftwareSigner::generate([7u8; 32]);
    assert_eq!(s.custody(), Custody::Software);
}

#[test]
fn a_signed_root_carries_its_custody_and_cannot_be_relabelled() {
    // `Custody` is set from the signer that produced the root and there is
    // no setter. A software build must not be able to emit a root claiming
    // hardware custody, and the type system is what enforces it rather than
    // a code review.
    let s = SoftwareSigner::generate([7u8; 32]);
    let signed = s.sign_root(&[0u8; 32], 0);
    assert_eq!(signed.custody, Custody::Software);
}

#[test]
fn the_signature_covers_both_the_root_and_the_size() {
    // A signature over the root alone lets a root be replayed at a different
    // claimed size, which is precisely the confusion C7.3.5 warns about.
    let s = SoftwareSigner::generate([7u8; 32]);
    let a = s.sign_root(&[1u8; 32], 8);
    let b = s.sign_root(&[1u8; 32], 9);
    assert_ne!(a.signature, b.signature);
}

#[test]
fn signing_is_deterministic_for_a_given_seed() {
    let a = SoftwareSigner::generate([7u8; 32]).sign_root(&[1u8; 32], 8);
    let b = SoftwareSigner::generate([7u8; 32]).sign_root(&[1u8; 32], 8);
    assert_eq!(a.signature, b.signature);
    assert_eq!(a.public_key, b.public_key);
}

#[test]
fn a_signed_root_renders_its_custody_and_the_undefended_property() {
    // The design requires that C7.3.3 be printed on every root this tool
    // publishes. A root that renders without saying so is the failure.
    let s = SoftwareSigner::generate([7u8; 32]);
    let text = s.sign_root(&[0u8; 32], 3).render();
    assert!(text.contains("SOFTWARE"));
    assert!(text.contains("C7.3.3"));
    assert!(text.to_lowercase().contains("equivocation"));
}
