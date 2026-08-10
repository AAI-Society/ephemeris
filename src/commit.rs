use crate::record::{Claim, Record};
use crate::store::Store;
use std::path::Path;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;

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

    /// Successful `fsync` calls made by the underlying store.
    pub fn sync_count(&self) -> u64 {
        self.store.sync_count()
    }

    /// Build records for `claims`, write them as one batch, and return one
    /// result per claim in submission order. Each claim carries its own
    /// `now_secs`, because a batch is assembled from submissions that arrived
    /// at different times.
    ///
    /// Every member of a failed batch fails. There is no partial success: the
    /// batch shares one `fsync`, so either all of its bytes are durable or
    /// none of them are known to be.
    ///
    /// **This is the only place an `Ack` is constructed in this crate.** The
    /// concurrent path in `CommitQueue` routes results from here back to
    /// submitters; it does not build them.
    pub fn submit_at(&mut self, claims: Vec<(Claim, u64)>) -> Vec<Result<Ack, CommitError>> {
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

        for (i, (c, now_secs)) in claims.into_iter().enumerate() {
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

    /// One timestamp for the whole batch. Kept because the CLI and the Phase 1
    /// tests submit a batch that was assembled at a single instant.
    pub fn submit(&mut self, claims: Vec<Claim>, now_secs: u64) -> Vec<Result<Ack, CommitError>> {
        self.submit_at(claims.into_iter().map(|c| (c, now_secs)).collect())
    }
}

/// One claim on its way to the log, with somewhere to send the answer.
struct Submission {
    claim: Claim,
    now_secs: u64,
    reply: SyncSender<Result<Ack, CommitError>>,
}

/// Work, or the instruction to stop.
///
/// Stopping is an explicit message rather than "the last sender was dropped".
/// Handles are cloneable and outlive the queue value, so waiting for every
/// sender to drop would let one forgotten handle hang `shutdown` forever. The
/// message travels the same channel as the work, so FIFO ordering guarantees
/// everything submitted before the stop is committed before the loop exits.
enum Msg {
    Submit(Box<Submission>),
    Stop,
}

/// What a completed run of the queue observed. Reported by `bench` so that a
/// claims/second figure can be checked against the number of times the disk
/// was actually made to commit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueStats {
    pub batches: u64,
    pub claims: u64,
    pub fsyncs: u64,
    pub batch_sizes: Vec<usize>,
}

/// A commit loop fed by a channel: many submitters, one batching thread, one
/// `fsync` per batch.
///
/// **The `Ack` guarantee crosses a thread boundary here and does not change.**
/// The loop calls `Committer::submit_at`, which is the single place an `Ack`
/// is constructed, and forwards the results to the submitters that are waiting
/// for them. A submitter *receives* an `Ack` and has no way to build one, so
/// "acknowledge before durable" remains inexpressible rather than merely
/// discouraged.
///
/// Batching policy is "take whatever is queued when the loop wakes". There is
/// no timer and no target batch size: a timer would trade latency for
/// throughput according to a constant we chose, which would make the benchmark
/// a measurement of that constant. Taking whatever has arrived makes batch size
/// an observable of the offered load instead.
pub struct CommitQueue {
    tx: Option<Sender<Msg>>,
    worker: Option<JoinHandle<QueueStats>>,
}

/// A cloneable submission handle. One per agent thread.
#[derive(Clone)]
pub struct QueueHandle {
    tx: Sender<Msg>,
}

impl QueueHandle {
    /// Submit one claim and block until the log has answered.
    ///
    /// Returns `NotDurable` rather than blocking forever if the commit loop is
    /// gone. A caller waiting on an acknowledgement that will never arrive is
    /// the same outage as a wrong answer and harder to diagnose.
    pub fn submit(&self, claim: Claim, now_secs: u64) -> Result<Ack, CommitError> {
        let (reply, answer) = sync_channel(1);
        let gone = || CommitError::NotDurable {
            why: "the commit loop is not running".to_string(),
        };
        self.tx
            .send(Msg::Submit(Box::new(Submission {
                claim,
                now_secs,
                reply,
            })))
            .map_err(|_| gone())?;
        answer.recv().map_err(|_| gone())?
    }
}

impl CommitQueue {
    pub fn new(committer: Committer) -> Self {
        let (tx, rx) = channel::<Msg>();
        let worker = std::thread::spawn(move || run_loop(committer, rx));
        Self {
            tx: Some(tx),
            worker: Some(worker),
        }
    }

    pub fn handle(&self) -> QueueHandle {
        QueueHandle {
            tx: self.tx.clone().unwrap_or_else(dead_sender),
        }
    }

    /// Stop accepting work, commit what is already queued, and report what
    /// happened.
    pub fn shutdown(mut self) -> QueueStats {
        self.stop()
    }

    fn stop(&mut self) -> QueueStats {
        if let Some(tx) = self.tx.take() {
            // A closed channel means the loop already exited; nothing to stop.
            let _ = tx.send(Msg::Stop);
        }
        match self.worker.take() {
            Some(w) => w.join().unwrap_or_default(),
            None => QueueStats::default(),
        }
    }
}

impl Drop for CommitQueue {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A sender whose receiver is already gone, handed out if `handle()` is somehow
/// called after the queue stopped. Every submission on it is refused rather
/// than panicking, so even a refactor that breaks the invariant fails closed.
fn dead_sender() -> Sender<Msg> {
    let (tx, rx) = channel();
    drop(rx);
    tx
}

fn run_loop(mut committer: Committer, rx: Receiver<Msg>) -> QueueStats {
    let mut stats = QueueStats::default();
    loop {
        // Block for the first message. `Err` means every handle is gone.
        let first = match rx.recv() {
            Ok(Msg::Submit(s)) => s,
            Ok(Msg::Stop) | Err(_) => break,
        };
        let mut batch = vec![first];
        // Take whatever else has already arrived. No timer: see the type docs.
        // A `Stop` found while draining ends the loop *after* this batch
        // commits, so nothing already submitted is dropped on the floor.
        let mut stopping = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Submit(s) => batch.push(s),
                Msg::Stop => {
                    stopping = true;
                    break;
                }
            }
        }

        let pairs: Vec<(Claim, u64)> = batch
            .iter()
            .map(|s| (s.claim.clone(), s.now_secs))
            .collect();
        let results = committer.submit_at(pairs);

        stats.batches += 1;
        stats.claims += batch.len() as u64;
        stats.batch_sizes.push(batch.len());
        stats.fsyncs = committer.sync_count();

        for (s, r) in batch.into_iter().zip(results) {
            // A submitter that has given up is not an error worth stopping
            // for; its record is durable either way.
            let _ = s.reply.send(r);
        }

        if stopping {
            break;
        }
    }
    stats
}
