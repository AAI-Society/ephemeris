use ed25519_dalek::pkcs8::EncodePublicKey;
use ed25519_dalek::{Signer as _, SigningKey};
use rand_core::SeedableRng;

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("could not encode the public key as a SubjectPublicKeyInfo: {0}")]
    Spki(String),
}

/// The DER encoding of the key's entire `SubjectPublicKeyInfo`.
///
/// **Not the bare key bits.** `parallax`'s verifier hashes the whole SPKI —
/// the `AlgorithmIdentifier` and the `subjectPublicKey` BIT STRING — and an
/// attester that hashed `verifying_key().to_bytes()` instead would produce a
/// `report_data` that never matches, failing only on real hardware against a
/// real verifier. `tests/tee_binding.rs` pins the distinction.
pub fn spki_der(key: &ed25519_dalek::VerifyingKey) -> Result<Vec<u8>, SignError> {
    key.to_public_key_der()
        .map(|d| d.as_bytes().to_vec())
        .map_err(|e| SignError::Spki(e.to_string()))
}

/// The discriminant behind `Custody`. Deliberately not `pub` — not even
/// `pub(crate)` — so nothing outside this module can name it, match on it,
/// or write a `CustodyInner::Tee` literal. `Custody` wraps it in a private
/// tuple field for the same reason: the earlier design made `Custody` a
/// `pub enum` and relied only on a feature-gated constructor to keep hardware
/// custody unreachable, but Rust cannot restrict one variant's visibility
/// below the enum's, so `Custody::Tee` remained a namable path from anywhere
/// that could see the `sign` module — feature on or off. A `SignedRoot {
/// custody: Custody::Tee, .. }` literal compiled outside this crate entirely,
/// with no `TeeSigner` and no attestation involved. Hiding the discriminant
/// behind an opaque struct is what actually closes that: there is no path
/// named `Custody::Tee` anywhere in the language now, in or outside this
/// crate, regardless of `tee`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CustodyInner {
    Software,
    /// Cfg-gated along with `Custody::tee()`, its only constructor. Without
    /// this, a default build would carry a `Tee` variant nothing ever
    /// constructs — dead code the compiler correctly warns about, and a
    /// value that, if the warning were silenced instead of heeded, would
    /// exist in the binary with no way to prove it should. Gating the
    /// variant keeps "does this build's `Custody` type even have a hardware
    /// case" answerable by `cfg`, matching the answer for whether it can be
    /// constructed.
    #[cfg(feature = "tee")]
    Tee,
}

/// Where the signing key lives.
///
/// Opaque by construction: both `CustodyInner` and this struct's field are
/// private, so there is no struct literal, no enum-variant path, and no
/// `match` arm outside this module that can produce or name a value of this
/// type — the only doors in are the constructors below. `software()` is
/// always available; `tee()` exists only when the `tee` feature is compiled
/// in. A default build therefore has no expression, anywhere in or outside
/// this crate, that evaluates to hardware custody — not merely no
/// *convenient* one. This mirrors `occultation`'s rule that a modelled
/// implementation cannot construct a valid verdict, and it is the same
/// discipline that keeps `Ack` unforgeable in `commit.rs`: a private field is
/// the enforcement mechanism, not an absent derive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Custody(CustodyInner);

impl Custody {
    /// Software custody. Always constructible — this is what every default
    /// build signs with.
    pub fn software() -> Custody {
        Custody(CustodyInner::Software)
    }

    pub fn label(&self) -> &'static str {
        match self.0 {
            CustodyInner::Software => "SOFTWARE",
            #[cfg(feature = "tee")]
            CustodyInner::Tee => "TEE",
        }
    }

    /// The only constructor for hardware custody, and it exists only when the
    /// `tee` feature is on. `pub(crate)` rather than `pub`, even under the
    /// feature: the sole caller is `TeeSigner::sign_root` in `tee.rs`, and
    /// nothing outside this crate should mint one directly.
    #[cfg(feature = "tee")]
    pub(crate) fn tee() -> Custody {
        Custody(CustodyInner::Tee)
    }
}

/// A signed root, together with everything a verifier needs to check it.
///
/// Every field is private. The only way to obtain one is [`Signer::sign_root`],
/// which funnels through [`SignedRoot::new`] — the single struct-literal site
/// in this crate — so a `SignedRoot` can never be assembled with a `custody`
/// that no `Signer` actually produced. Before this, every field here was
/// `pub` and `Custody`'s variant was a namable path, which together let code
/// outside this crate write `SignedRoot { custody: Custody::Tee, .. }`
/// directly and render a hardware-custody claim no `TeeSigner` had ever
/// produced — see the `compile_fail` example on `render` below, which is
/// exactly that literal and now cannot compile.
#[derive(Clone, Debug)]
pub struct SignedRoot {
    root: String,
    size: u64,
    public_key: String,
    signature: String,
    custody: Custody,
}

impl SignedRoot {
    /// The only place this struct's literal is written. `pub(crate)` so both
    /// `SoftwareSigner` (below) and `TeeSigner` (`tee.rs`) can call it while
    /// it stays invisible outside this crate: nothing downstream can build a
    /// `SignedRoot` except by holding an actual `Signer` and calling
    /// `sign_root`.
    pub(crate) fn new(
        root: String,
        size: u64,
        public_key: String,
        signature: String,
        custody: Custody,
    ) -> SignedRoot {
        SignedRoot {
            root,
            size,
            public_key,
            signature,
            custody,
        }
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn custody(&self) -> Custody {
        self.custody
    }

    /// Every root this tool publishes says what it does not defend against.
    ///
    /// C7.3.3 is unmet in Phase 1: nothing is published outside the
    /// operator's control, so an operator holding this key can alter a past
    /// record, recompute every link, re-sign, and produce a log that replays
    /// perfectly. Only a root published before the rewrite contradicts it.
    /// Printing this is the same discipline as `poc-audit` shipping a
    /// permanent `UNCHECKED` row.
    ///
    /// # The forgery this struct's privacy makes impossible
    ///
    /// This is the literal a caller outside this crate could write before
    /// `SignedRoot`'s fields and `Custody`'s discriminant were made private —
    /// no `TeeSigner`, no attestation, just a struct literal. It no longer
    /// compiles:
    ///
    /// ```compile_fail
    /// use ephemeris::sign::{Custody, SignedRoot};
    /// let forged = SignedRoot {
    ///     root: "00".repeat(32),
    ///     size: 1,
    ///     public_key: "aa".repeat(32),
    ///     signature: "bb".repeat(64),
    ///     custody: Custody::Tee,
    /// };
    /// ```
    pub fn render(&self) -> String {
        format!(
            "root {} size {} key {} custody {}\n\
             NOT DEFENDED — C7.3.3 equivocation: this root is published only by its own \
             operator. An operator holding the signing key can rewrite history and re-sign it. \
             Compare against a root you obtained earlier and independently, or this signature \
             establishes only that the log is internally consistent.",
            self.root,
            self.size,
            self.public_key,
            self.custody.label()
        )
    }
}

pub trait Signer {
    fn sign_root(&self, root: &[u8; 32], size: u64) -> SignedRoot;
    fn custody(&self) -> Custody;
}

/// The Phase 1 signer. Deterministic from a seed so tests are reproducible
/// and so the same log signed twice is byte-identical.
pub struct SoftwareSigner {
    key: SigningKey,
}

impl SoftwareSigner {
    /// Derive the signing key from `seed` with a seeded RNG.
    ///
    /// This determinism is deliberate, unlike `TeeSigner::generate`'s former
    /// seed parameter (removed — see `tee.rs`). The two cases are not
    /// symmetric: a software key makes no custody claim beyond "the
    /// operator holds it", which is exactly as true of a key derived from a
    /// constant as of one drawn from the OS CSPRNG — `Custody::software()`'s
    /// rendering already says "no defence against the operator", so a
    /// predictable key betrays nothing the label did not already concede.
    /// Reproducibility is worth having here: it makes the same log signed
    /// twice byte-identical, which is useful for tests and for anyone
    /// diffing output. A `TeeSigner`, by contrast, renders `custody TEE`, a
    /// claim that a predictable key would falsify outright, so it was made
    /// unforgeable at the type level instead of merely documented against.
    pub fn generate(seed: [u8; 32]) -> Self {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
        Self {
            key: SigningKey::generate(&mut rng),
        }
    }

    /// The public half, for callers that need to derive a binding from it.
    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.key.verifying_key()
    }
}

/// What a root signature covers: the domain tag, the size, and the root.
///
/// The size is inside the signed bytes deliberately. A signature over the
/// root alone would let a valid root be presented at a different claimed
/// size, and a verifier comparing sizes rather than roots is exactly the
/// defect C7.3.5 was written about.
pub(crate) fn signing_bytes(root: &[u8; 32], size: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + 8 + 32);
    v.extend_from_slice(b"eph-root");
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(root);
    v
}

impl Signer for SoftwareSigner {
    fn sign_root(&self, root: &[u8; 32], size: u64) -> SignedRoot {
        let sig = self.key.sign(&signing_bytes(root, size));
        SignedRoot::new(
            hex::encode(root),
            size,
            hex::encode(self.key.verifying_key().to_bytes()),
            hex::encode(sig.to_bytes()),
            Custody::software(),
        )
    }

    fn custody(&self) -> Custody {
        Custody::software()
    }
}
