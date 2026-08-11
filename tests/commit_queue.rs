use ephemeris::commit::{CommitError, CommitQueue, Committer};
use ephemeris::record::Claim;
use ephemeris::store::Store;
use std::collections::BTreeSet;

fn claim(id: &str) -> Claim {
    Claim {
        action_id: id.into(),
        agent_id: "did:web:example.org:agents:ref-1".into(),
        initiating_user: "user:alice".into(),
        interception_point: "PRE_CALL_TOOL_INVOCATION".into(),
        target_resource: "/v1/charges".into(),
        canonical_snapshot_hash: "54c323d3".into(),
        path_summary_hash: None,
        policy_bundle_hash: "a7713be5".into(),
        verdict: "ALLOW".into(),
        nonce: "n-1".into(),
    }
}

#[test]
fn every_claim_gets_exactly_one_ack_and_all_are_durable() {
    let d = tempfile::tempdir().unwrap();
    let q = CommitQueue::new(Committer::new(Store::open(d.path()).unwrap()));

    let mut threads = Vec::new();
    for a in 0..8 {
        let h = q.handle();
        threads.push(std::thread::spawn(move || {
            let mut acks = Vec::new();
            for i in 0..25 {
                acks.push(h.submit(claim(&format!("a-{a}-{i}")), 1_700_000_000));
            }
            acks
        }));
    }
    let acks: Vec<_> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();

    assert_eq!(acks.len(), 200);
    assert!(acks.iter().all(|a| a.is_ok()), "a claim was refused");

    // Exactly one ack per claim means the step indices are a permutation of
    // 0..200 with no duplicate and no gap. A duplicate would mean two claims
    // were told they occupy the same position in history.
    let steps: BTreeSet<u64> = acks
        .iter()
        .map(|a| a.as_ref().unwrap().step_index())
        .collect();
    assert_eq!(
        steps.len(),
        200,
        "two claims were given the same step index"
    );
    assert_eq!(steps.iter().next(), Some(&0));
    assert_eq!(steps.iter().next_back(), Some(&199));

    let stats = q.shutdown();
    assert_eq!(stats.claims, 200);
    assert_eq!(stats.fsyncs, stats.batches, "one fsync per batch, exactly");
    assert!(stats.batches <= 200);

    // Everything acked must survive a reopen. Anything else means an ack
    // preceded durability.
    let reopened = Store::open(d.path()).unwrap();
    assert_eq!(reopened.records().len(), 200);
    ephemeris::chain::verify_sequence(reopened.records()).expect("chain must verify");
}

#[test]
fn group_commit_actually_batches_under_concurrent_load() {
    // If every claim got its own fsync the queue would be pointless. This does
    // not assert a batch size — that is load- and machine-dependent — only
    // that batching happened at all under 8 concurrent submitters.
    let d = tempfile::tempdir().unwrap();
    let q = CommitQueue::new(Committer::new(Store::open(d.path()).unwrap()));
    let mut threads = Vec::new();
    for a in 0..8 {
        let h = q.handle();
        threads.push(std::thread::spawn(move || {
            for i in 0..25 {
                let _ = h.submit(claim(&format!("a-{a}-{i}")), 1_700_000_000);
            }
        }));
    }
    for t in threads {
        t.join().unwrap();
    }
    let stats = q.shutdown();
    assert!(
        stats.batches < stats.claims,
        "no batching occurred: {} batches for {} claims",
        stats.batches,
        stats.claims
    );
}

#[test]
fn an_unwritable_store_acks_nothing_from_any_submitter() {
    // Phase 1's fail-closed rule, now across a thread boundary. Every
    // submitter must be refused, not merely the one that happened to be
    // first.
    let d = tempfile::tempdir().unwrap();
    let mut c = Committer::new(Store::open(d.path()).unwrap());
    let seg = d.path().join("segment-0.log");
    std::fs::remove_file(&seg).unwrap();
    std::fs::create_dir(&seg).unwrap();
    c.force_reopen_for_test(d.path());

    let q = CommitQueue::new(c);
    let mut threads = Vec::new();
    for a in 0..4 {
        let h = q.handle();
        threads.push(std::thread::spawn(move || {
            (0..5)
                .map(|i| h.submit(claim(&format!("a-{a}-{i}")), 1_700_000_000))
                .collect::<Vec<_>>()
        }));
    }
    let results: Vec<_> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();

    assert_eq!(results.len(), 20);
    for r in &results {
        assert!(
            matches!(r, Err(CommitError::NotDurable { .. })),
            "a claim was acked without being durable"
        );
    }
    let stats = q.shutdown();
    assert_eq!(stats.fsyncs, 0, "an unwritable store performed a sync");
}

#[test]
fn a_sync_failure_after_earlier_successes_does_not_corrupt_them() {
    // The commit loop submits many batches per process, unlike Phase 1's
    // one-batch-per-process CLI. `append_batch` writes its bytes for real
    // and only then fails the sync — so if the committer is not latched
    // `unavailable` after that, the loop will build the next batch from the
    // same stale `first_step`/`head` as before the failure (nothing updated
    // them, since `append_batch` only advances in-memory state after both
    // the write and the sync succeed) and, if that retry happens to
    // succeed, append a second, conflicting record at the same
    // `step_index` right after the first — corrupting the chain, not just
    // failing to record the retry.
    let d = tempfile::tempdir().unwrap();
    let mut c = Committer::new(Store::open(d.path()).unwrap());

    // A batch succeeds first, through the same `submit` the queue's loop
    // calls internally.
    let setup = c.submit(vec![claim("a-0"), claim("a-1")], 1_700_000_000);
    assert!(setup.iter().all(|a| a.is_ok()), "setup batch was refused");

    // Make the *next* batch land its bytes for real and then fail as if
    // `fsync` had failed.
    c.force_next_sync_to_fail_for_test();

    let q = CommitQueue::new(c);
    let h = q.handle();

    // This is the claim whose batch hits the simulated sync failure. It
    // must be refused. `submit` blocks until the commit loop has answered,
    // so by the time this returns the loop has already processed it as its
    // own batch (nothing else was queued yet) and is back to waiting for
    // the next message — the subsequent submissions below cannot race it.
    let phantom = h.submit(claim("b-0"), 1_700_000_000);
    assert!(
        matches!(phantom, Err(CommitError::NotDurable { .. })),
        "a claim was acked whose write was never synced"
    );

    // Everything submitted after the sync failure. Without the latch, some
    // of these would succeed — reusing the pre-failure `step_index` and
    // producing a duplicate — rather than being refused.
    let mut threads = Vec::new();
    for a in 0..4 {
        let h = q.handle();
        threads.push(std::thread::spawn(move || {
            (0..3)
                .map(|i| h.submit(claim(&format!("c-{a}-{i}")), 1_700_000_000))
                .collect::<Vec<_>>()
        }));
    }
    let results: Vec<_> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();

    // (a) every claim submitted after the sync failure is refused, not just
    // the one batch that hit it.
    assert_eq!(results.len(), 12);
    for r in &results {
        assert!(
            matches!(r, Err(CommitError::NotDurable { .. })),
            "a claim was acked after an earlier batch's sync failed"
        );
    }
    let _ = q.shutdown();

    // (b) the property that matters: a fresh open still succeeds and the
    // chain of whatever landed on disk is internally consistent — no
    // duplicate or gapped `step_index`. The record whose sync failed is
    // present (its bytes really were written) but was never acked to any
    // caller; that is correct, not corruption — a sync failing in a process
    // that does not crash is not data loss, and the store does not need to
    // hide bytes that are genuinely on disk. What it must never do is place
    // a second, conflicting record after them, and it does not.
    let reopened = Store::open(d.path()).unwrap();
    ephemeris::chain::verify_sequence(reopened.records()).expect("chain must verify");
    assert_eq!(
        reopened.records().len(),
        3,
        "2 acked plus 1 unacked-but-durable"
    );
}

#[test]
fn a_submitter_whose_queue_is_gone_is_refused_not_hung() {
    // If the commit loop dies, a submitter must get an error rather than
    // block forever. A gateway waiting on an ack that never arrives is the
    // same outage as a wrong answer, and harder to diagnose.
    let d = tempfile::tempdir().unwrap();
    let q = CommitQueue::new(Committer::new(Store::open(d.path()).unwrap()));
    let h = q.handle();
    let _ = q.shutdown();
    assert!(matches!(
        h.submit(claim("a-0"), 1_700_000_000),
        Err(CommitError::NotDurable { .. })
    ));
}
