//! The TEE signer. Behind the `tee` feature, because it needs `parallax` and
//! a configfs-tsm report directory that only a TDX guest has.

use crate::sign::{signing_bytes, spki_der, Custody, SignError, SignedRoot, Signer};
use ed25519_dalek::{Signer as _, SigningKey};
use rand_core::SeedableRng;

#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    #[error("could not derive the key binding: {0}")]
    Binding(#[from] SignError),
    #[error("the platform would not produce a quote over this key: {0}")]
    Quote(String),
}

/// A signing key bound to a quote through `report_data`.
///
/// The binding is `parallax::ratls::expected_report_data(spki)`, which is
/// `SHA-256(SPKI DER)` left-aligned into 64 bytes with the remainder zero.
/// **We call parallax's function rather than reproducing that layout**: the
/// verifier on the other side is parallax's, and two implementations of one
/// identity derivation is the failure this programme keeps finding in other
/// people's work.
pub struct TeeSigner {
    key: SigningKey,
    quote: Vec<u8>,
}

impl TeeSigner {
    /// Generate a key and obtain a quote committing to it.
    ///
    /// Fails closed: no quote, no signer. A `TeeSigner` that existed without a
    /// quote could sign roots claiming hardware custody it could not
    /// demonstrate, which is precisely the claim this type exists to make
    /// checkable.
    pub fn generate(seed: [u8; 32]) -> Result<Self, TeeError> {
        Self::generate_at(
            std::path::Path::new(parallax::attest::tsm::TSM_REPORT_DIR),
            seed,
        )
    }

    /// [`TeeSigner::generate`], against a caller-supplied report directory.
    ///
    /// Split out for the same reason parallax split `request_quote_at`: the
    /// real path is `/sys`, which no test can create, so the failure paths
    /// would otherwise be untested — and the failure paths are where
    /// fail-closed either holds or does not.
    pub fn generate_at(base: &std::path::Path, seed: [u8; 32]) -> Result<Self, TeeError> {
        let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
        let key = SigningKey::generate(&mut rng);
        let spki = spki_der(&key.verifying_key())?;
        let report_data = parallax::ratls::expected_report_data(&spki);
        let quote = parallax::attest::tsm::request_quote_at(base, &report_data)
            .map_err(|e| TeeError::Quote(e.to_string()))?;
        Ok(Self { key, quote })
    }

    pub fn quote(&self) -> &[u8] {
        &self.quote
    }
}

impl Signer for TeeSigner {
    fn sign_root(&self, root: &[u8; 32], size: u64) -> SignedRoot {
        let sig = self.key.sign(&signing_bytes(root, size));
        SignedRoot::new(
            hex::encode(root),
            size,
            hex::encode(self.key.verifying_key().to_bytes()),
            hex::encode(sig.to_bytes()),
            Custody::tee(),
        )
    }

    fn custody(&self) -> Custody {
        Custody::tee()
    }
}
