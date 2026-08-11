#![cfg(feature = "tee")]

use ephemeris::tee::{TeeError, TeeSigner};

#[test]
fn no_report_directory_means_no_signer() {
    // The developer-laptop case, and the one that must fail closed. An empty
    // directory is not a TDX guest.
    let d = tempfile::tempdir().unwrap();
    let r = TeeSigner::generate_at(d.path());
    assert!(
        matches!(r, Err(TeeError::Quote(_))),
        "a machine with no TEE produced a signer"
    );
}

#[test]
fn the_error_names_the_platform_not_the_key() {
    // A missing TEE is an environment problem. The message must not suggest
    // the key was at fault, or an operator will go looking in the wrong place.
    let d = tempfile::tempdir().unwrap();
    let msg = match TeeSigner::generate_at(d.path()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a temp dir is not a TEE"),
    };
    assert!(
        msg.contains("quote"),
        "message did not mention the quote: {msg}"
    );
}

#[test]
fn a_failed_quote_leaves_no_signer_to_misuse() {
    // Fail-closed, stated as a type property: `generate_at` returns Result and
    // there is no other constructor, so a failed attestation cannot leave a
    // TeeSigner in scope that would sign roots claiming hardware custody.
    let d = tempfile::tempdir().unwrap();
    assert!(TeeSigner::generate_at(d.path()).is_err());
}

#[test]
fn a_missing_base_directory_is_also_not_a_tee() {
    // Distinct from an empty directory: this path exercises
    // `report_dir_available_at`'s "absent" branch rather than the "exists but
    // has none of configfs-tsm's attributes" branch a bare tempdir hits. Two
    // different reasons to fail closed should both actually fail closed.
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("does-not-exist");
    let r = TeeSigner::generate_at(&missing);
    assert!(
        matches!(r, Err(TeeError::Quote(_))),
        "a nonexistent report directory produced a signer"
    );
}

#[test]
fn an_unwritable_report_directory_is_not_a_tee() {
    // A third distinct code path: the base directory exists and is visible,
    // but nothing can be created under it, so quote generation fails at
    // `ReportEntry::create` rather than at the availability check.
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = TeeSigner::generate_at(d.path());
    // Restore write permission so `TempDir`'s own cleanup can remove it.
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(r, Err(TeeError::Quote(_))),
        "an unwritable report directory produced a signer"
    );
}
