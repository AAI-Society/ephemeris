use ed25519_dalek::{Signer as _, SigningKey};
use rand_core::SeedableRng;

/// Where the signing key lives. Set by the signer that produced a root, with
/// no setter anywhere, so a software build cannot emit a root claiming
/// hardware custody. This mirrors `occultation`'s rule that a modelled
/// implementation cannot construct a valid verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Custody {
    /// The key is a process-local Ed25519 key. It provides integrity against
    /// anyone who is not the operator, and nothing against the operator.
    Software,
    /// The key was generated inside a TEE and is bound to a quote through
    /// `REPORTDATA`. Phase 2.
    Tee,
}

impl Custody {
    pub fn label(&self) -> &'static str {
        match self {
            Custody::Software => "SOFTWARE",
            Custody::Tee => "TEE",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SignedRoot {
    pub root: String,
    pub size: u64,
    pub public_key: String,
    pub signature: String,
    pub custody: Custody,
}

impl SignedRoot {
    /// Every root this tool publishes says what it does not defend against.
    ///
    /// C7.3.3 is unmet in Phase 1: nothing is published outside the
    /// operator's control, so an operator holding this key can alter a past
    /// record, recompute every link, re-sign, and produce a log that replays
    /// perfectly. Only a root published before the rewrite contradicts it.
    /// Printing this is the same discipline as `poc-audit` shipping a
    /// permanent `UNCHECKED` row.
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
    pub fn generate(seed: [u8; 32]) -> Self {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
        Self {
            key: SigningKey::generate(&mut rng),
        }
    }
}

/// What a root signature covers: the domain tag, the size, and the root.
///
/// The size is inside the signed bytes deliberately. A signature over the
/// root alone would let a valid root be presented at a different claimed
/// size, and a verifier comparing sizes rather than roots is exactly the
/// defect C7.3.5 was written about.
fn signing_bytes(root: &[u8; 32], size: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + 8 + 32);
    v.extend_from_slice(b"eph-root");
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(root);
    v
}

impl Signer for SoftwareSigner {
    fn sign_root(&self, root: &[u8; 32], size: u64) -> SignedRoot {
        let sig = self.key.sign(&signing_bytes(root, size));
        SignedRoot {
            root: hex::encode(root),
            size,
            public_key: hex::encode(self.key.verifying_key().to_bytes()),
            signature: hex::encode(sig.to_bytes()),
            custody: Custody::Software,
        }
    }

    fn custody(&self) -> Custody {
        Custody::Software
    }
}
