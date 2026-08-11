//! The property the design asks for: a build without the `tee` feature cannot
//! emit a root claiming hardware custody.
//!
//! # Where the real proof lives
//!
//! This file used to also contain `nothing_in_a_default_build_constructs_tee_custody`,
//! a text scan of `src/sign.rs` that demanded every line containing
//! `Custody::Tee` be a match arm or carry the `tee` cfg attribute. It was
//! deleted, not weakened, because it was checking the wrong thing: it only
//! ever read `src/sign.rs`'s own source, so it was structurally blind to a
//! `SignedRoot { custody: Custody::Tee, .. }` literal written *anywhere
//! else* — another file in this crate, or a downstream crate entirely. That
//! is exactly where the real hole was. Before this file's fix, `Custody` was
//! `pub enum Custody { Software, Tee }` and every field of `SignedRoot` was
//! `pub`. Rust cannot restrict one enum variant's visibility below the
//! enum's, so `Custody::Tee` was a namable path — and `SignedRoot`'s public
//! fields meant no `Signer` was needed to assemble one. The following
//! compiled and rendered a full hardware-custody claim with no `TeeSigner`
//! and no `tee` feature:
//!
//! ```rust,ignore
//! use ephemeris::sign::{Custody, SignedRoot};
//! let forged = SignedRoot {
//!     root: "00".repeat(32), size: 1,
//!     public_key: "aa".repeat(32), signature: "bb".repeat(64),
//!     custody: Custody::Tee,
//! };
//! forged.render() // "... custody TEE"
//! ```
//!
//! The fix makes both types opaque outside `src/sign.rs`: `Custody` wraps a
//! private `CustodyInner` enum, and `SignedRoot`'s fields are private with
//! read accessors, constructed only through `SignedRoot::new` (itself
//! `pub(crate)`, called only by `Signer::sign_root` implementations). That
//! turns the snippet above into a compile error rather than a passing test —
//! see the `compile_fail` doctest on `SignedRoot::render` in `src/sign.rs`,
//! which is that exact snippet, checked by `cargo test` (doc-tests run by
//! default) every time this crate is tested. A text scan can only ever tell
//! you a particular file looks a certain way; the compiler is what actually
//! enforces "there is no expression, anywhere, that produces this value,"
//! and only a type-level fix lets the compiler do that job.
//!
//! What is left here is behavioural: what a real `SoftwareSigner` actually
//! produces and renders.

use ephemeris::sign::{Custody, Signer, SoftwareSigner};

#[test]
fn a_default_build_signs_with_software_custody() {
    let s = SoftwareSigner::generate([7u8; 32]);
    assert_eq!(s.custody(), Custody::software());
    assert_eq!(s.sign_root(&[0u8; 32], 1).custody(), Custody::software());
}

#[test]
fn a_rendered_root_says_software_and_says_what_is_undefended() {
    let text = SoftwareSigner::generate([7u8; 32])
        .sign_root(&[0u8; 32], 1)
        .render();
    assert!(text.contains("SOFTWARE"));
    assert!(
        !text.contains("TEE"),
        "a software build rendered a TEE claim"
    );
    assert!(text.contains("C7.3.3"));
}
