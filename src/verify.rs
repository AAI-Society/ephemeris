use crate::chain::{verify_sequence, ChainError};
use crate::record::{digest, Record, RecordError};
use crate::tree::{consistency_proof, leaf_hash, root_of, verify_consistency};

/// A root published before now, against which a log claims to be an
/// append-only extension. Obtaining one is outside this crate's scope in
/// Phase 1 — see the design's "what this does not defend against".
#[derive(Clone, Copy, Debug)]
pub struct PriorRoot {
    pub size: u64,
    pub root: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
pub struct Accepted {
    pub size: u64,
    pub root: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("the log is not internally consistent: {0}")]
    Chain(#[from] ChainError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error(
        "the log is not an append-only extension of the root published at size {prior_size}; \
         a record before that point was altered and the chain relinked"
    )]
    Inconsistent { prior_size: u64 },
    #[error("the prior root claims size {prior_size}, larger than this log's {size}")]
    PriorLarger { prior_size: u64, size: u64 },
}

/// Verify a log, and — when a prior root is supplied — verify that it did not
/// rewrite history.
///
/// **`prior` is `Option` and that is the whole design.** Passing `None` runs
/// only the internal checks, which a rewritten log passes. This is not an
/// oversight to be tightened later; it is the honest shape of a tool that
/// publishes no root outside the operator's control. The caller must decide
/// whether it holds a prior root, and `None` means it does not.
pub fn against_root(records: &[Record], prior: Option<PriorRoot>) -> Result<Accepted, VerifyError> {
    verify_sequence(records)?;

    let mut leaves = Vec::with_capacity(records.len());
    for r in records {
        leaves.push(leaf_hash(&digest(r)?));
    }
    let root = root_of(&leaves);
    let size = leaves.len() as u64;

    if let Some(p) = prior {
        if p.size > size {
            return Err(VerifyError::PriorLarger {
                prior_size: p.size,
                size,
            });
        }
        let proof = consistency_proof(&leaves, p.size as usize)
            .ok_or(VerifyError::Inconsistent { prior_size: p.size })?;
        if !verify_consistency(p.size, p.root, size, root, &proof) {
            return Err(VerifyError::Inconsistent { prior_size: p.size });
        }
    }

    Ok(Accepted { size, root })
}
