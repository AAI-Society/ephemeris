use crate::record::{digest, Record, RecordError};
use sha2::{Digest, Sha256};

/// The head a chain starts from. All-zero, so a first record's
/// `previous_head` is unambiguous and cannot be confused with a real digest
/// that happened to be produced by an empty input.
pub const GENESIS: [u8; 32] = [0u8; 32];

/// `chain_head_n = H(chain_head_{n-1} ‖ digest(record_n))`.
///
/// **This differs from the schema, deliberately.** The evidence schema defines
/// `chain_head` as `H(previous_head ‖ canonical_snapshot_hash ‖ verdict)` —
/// three fields of the record, not the record. Chaining over only those three
/// leaves every other field unprotected by the chain: `step_index`,
/// `target_resource`, `policy_bundle_hash` and the identity fields could all be
/// altered without breaking a single link.
///
/// Extending with the whole record digest is strictly stronger and costs
/// nothing, since the digest is already computed for the tree leaf. The
/// weaker schema definition is a finding to report upstream rather than a
/// constraint to honour; see the design's "specification gap" section.
pub fn extend(previous_head: [u8; 32], r: &Record) -> Result<[u8; 32], RecordError> {
    let mut h = Sha256::new();
    h.update(previous_head);
    h.update(digest(r)?);
    Ok(h.finalize().into())
}

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("record {at} declares chain_head {found}, but the chain computes {expected}")]
    HeadMismatch {
        at: usize,
        expected: String,
        found: String,
    },
    #[error("step_index jumps from {expected} to {found}; C7.3.1 treats a gap as an omission")]
    StepGap { expected: u64, found: u64 },
    #[error(transparent)]
    Record(#[from] RecordError),
}

/// Verify that a run of records is internally consistent: each declares the
/// head its predecessor produced, and step indices are contiguous from zero.
///
/// This establishes that **nobody altered the log without re-signing it**. It
/// says nothing about an operator who altered a record and recomputed every
/// subsequent link, which is attack A9 and is `verify::against_root`'s job.
///
/// `step_index` is checked **before** `chain_head`. A removed record leaves its
/// successor holding both a stale head and a wrong index; the gap is the more
/// specific diagnosis, so it wins.
pub fn verify_sequence(records: &[Record]) -> Result<(), ChainError> {
    let mut head = GENESIS;
    for (i, r) in records.iter().enumerate() {
        let expected_step = i as u64;
        if r.step_index != expected_step {
            return Err(ChainError::StepGap {
                expected: expected_step,
                found: r.step_index,
            });
        }
        let want = hex::encode(head);
        if r.chain_head != want {
            return Err(ChainError::HeadMismatch {
                at: i,
                expected: want,
                found: r.chain_head.clone(),
            });
        }
        head = extend(head, r)?;
    }
    Ok(())
}
