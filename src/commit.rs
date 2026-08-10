use crate::record::{Claim, Record};
use crate::store::Store;
use std::path::Path;

/// What a caller receives once, and only once, its record is durable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ack {
    pub step_index: u64,
    pub chain_head: String,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CommitError {
    #[error("the claim was not durably written and must not be forwarded: {why}")]
    NotDurable { why: String },
}

/// Batches claims into a single `fsync`.
///
/// **The `Ack` type is the enforcement mechanism.** It is constructed in
/// exactly one place — after `append_batch` returns `Ok` — so there is no
/// code path that produces one for a claim whose bytes are not on disk. A
/// boolean return would have made the mistake expressible; this does not.
pub struct Committer {
    store: Store,
    /// Set when the store could not be re-opened. Every `submit` then refuses.
    ///
    /// This is not a test affordance. An open file descriptor survives its
    /// path being unlinked, so a committer that has lost its store keeps
    /// accepting writes into an inode nothing can ever read back — bytes that
    /// are durable in the narrowest sense and evidence in no sense at all.
    /// Acking those is precisely the fail-open behaviour C7.1.3 forbids, so a
    /// committer that cannot see its own store refuses instead.
    unavailable: bool,
}

impl Committer {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            unavailable: false,
        }
    }

    pub fn next_step(&self) -> u64 {
        self.store.next_step()
    }

    /// Test-only hook: re-open the store so a test can break the underlying
    /// file and observe the failure path. Not part of the serving API.
    ///
    /// A failed re-open marks the committer unavailable rather than being
    /// discarded. Discarding it would leave the stale descriptor in place and
    /// the next batch would ack against an unlinked file.
    #[doc(hidden)]
    pub fn force_reopen_for_test(&mut self, dir: &Path) {
        match Store::open(dir) {
            Ok(s) => {
                self.store = s;
                self.unavailable = false;
            }
            Err(_) => self.unavailable = true,
        }
    }

    /// Build records for `claims`, write them as one batch, and return one
    /// result per claim in submission order.
    ///
    /// Every member of a failed batch fails. There is no partial success: the
    /// batch shares one `fsync`, so either all of its bytes are durable or
    /// none of them are known to be.
    pub fn submit(&mut self, claims: Vec<Claim>, now_secs: u64) -> Vec<Result<Ack, CommitError>> {
        let n = claims.len();
        if self.unavailable {
            return vec![
                Err(CommitError::NotDurable {
                    why: "the evidence store could not be opened".to_string()
                });
                n
            ];
        }
        let mut records = Vec::with_capacity(n);
        let first_step = self.store.next_step();
        let mut head = self.store.head();

        for (i, c) in claims.into_iter().enumerate() {
            let r = Record::new(c, first_step + i as u64, head, now_secs);
            match crate::chain::extend(head, &r) {
                Ok(next) => head = next,
                Err(e) => return vec![Err(CommitError::NotDurable { why: e.to_string() }); n],
            }
            records.push(r);
        }

        match self.store.append_batch(&records) {
            Ok(()) => records
                .iter()
                .map(|r| {
                    Ok(Ack {
                        step_index: r.step_index,
                        chain_head: r.chain_head.clone(),
                    })
                })
                .collect(),
            Err(e) => vec![Err(CommitError::NotDurable { why: e.to_string() }); n],
        }
    }
}
