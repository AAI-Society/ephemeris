//! The TEE signer. Behind the `tee` feature, because it needs `parallax` and
//! a configfs-tsm report directory that only a TDX guest has.

use crate::sign::{signing_bytes, spki_der, Custody, SignError, SignedRoot, Signer};
use ed25519_dalek::{Signer as _, SigningKey};
use rand_core::OsRng;

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
    ///
    /// # No seed, by design
    ///
    /// There is no way to pass this constructor a seed, and there will not
    /// be one added. A deterministic TEE key is a contradiction in terms:
    /// the whole premise of this type is that the signing key is generated
    /// inside the TEE and never leaves it, and a key reproducible from
    /// caller-supplied bytes was never inside anything — it was inside
    /// whatever process chose those bytes. An earlier version of this
    /// constructor took a `seed: [u8; 32]` and expanded it with a seeded
    /// RNG; the quote bound the *public* half, so a constant seed produced a
    /// key everyone who could read the constant also held, and the platform
    /// attested it anyway, `parallax` verified the binding anyway, and every
    /// root rendered `custody TEE` anyway. The key is now drawn from the
    /// operating system's CSPRNG (`OsRng`) instead, which closes the hole at
    /// the type level rather than documenting around it — the same
    /// discipline `Custody` and `SignedRoot` already use elsewhere in this
    /// crate: make the wrong thing unrepresentable, don't just warn about it.
    pub fn generate() -> Result<Self, TeeError> {
        Self::generate_at(std::path::Path::new(parallax::attest::tsm::TSM_REPORT_DIR))
    }

    /// [`TeeSigner::generate`], against a caller-supplied report directory.
    ///
    /// Split out for the same reason parallax split `request_quote_at`: the
    /// real path is `/sys`, which no test can create, so the failure paths
    /// would otherwise be untested — and the failure paths are where
    /// fail-closed either holds or does not.
    ///
    /// No seed parameter here either, for the same reason as
    /// [`TeeSigner::generate`]: the key comes from `OsRng` unconditionally.
    /// This entry point exists to let a caller control the report
    /// directory, not the key material.
    pub fn generate_at(base: &std::path::Path) -> Result<Self, TeeError> {
        let key = SigningKey::generate(&mut OsRng);
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
