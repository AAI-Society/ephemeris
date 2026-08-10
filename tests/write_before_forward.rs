use ephemeris::commit::{CommitError, Committer};
use ephemeris::record::Claim;
use ephemeris::store::Store;

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
fn a_batch_is_acked_only_after_it_is_durable() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    let mut c = Committer::new(store);

    let claims = vec![claim("a-0"), claim("a-1"), claim("a-2")];
    let acks = c.submit(claims, 1_700_000_000);
    assert_eq!(acks.len(), 3);
    for a in &acks {
        assert!(a.is_ok());
    }

    // Everything acked must be readable from a fresh open of the same
    // directory. Anything else means the ack preceded durability.
    let reopened = Store::open(d.path()).unwrap();
    assert_eq!(reopened.records().len(), 3);
}

#[test]
fn acks_carry_contiguous_step_indices_in_submission_order() {
    let d = tempfile::tempdir().unwrap();
    let mut c = Committer::new(Store::open(d.path()).unwrap());
    let acks = c.submit(
        vec![claim("a-0"), claim("a-1"), claim("a-2")],
        1_700_000_000,
    );
    let steps: Vec<u64> = acks
        .iter()
        .map(|a| a.as_ref().unwrap().step_index)
        .collect();
    assert_eq!(steps, vec![0, 1, 2]);
}

#[test]
fn an_unwritable_store_acks_nothing() {
    // C7.1.3's own auditor test: make the evidence store unavailable and
    // observe that actions do not proceed. Here that means every member of
    // the batch fails; a partial success would let a gateway forward an
    // action whose record was never written.
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    let mut c = Committer::new(store);

    // Replace the segment with a directory, so every subsequent write fails.
    let seg = d.path().join("segment-0.log");
    std::fs::remove_file(&seg).unwrap();
    std::fs::create_dir(&seg).unwrap();
    c.force_reopen_for_test(d.path());

    let acks = c.submit(vec![claim("a-0"), claim("a-1")], 1_700_000_000);
    assert_eq!(acks.len(), 2);
    for a in &acks {
        assert!(
            matches!(a, Err(CommitError::NotDurable { .. })),
            "a claim was acked without being durable"
        );
    }
}

#[test]
fn a_failed_batch_does_not_advance_the_chain() {
    let d = tempfile::tempdir().unwrap();
    let mut c = Committer::new(Store::open(d.path()).unwrap());
    let before = c.next_step();
    let seg = d.path().join("segment-0.log");
    std::fs::remove_file(&seg).unwrap();
    std::fs::create_dir(&seg).unwrap();
    c.force_reopen_for_test(d.path());
    let _ = c.submit(vec![claim("a-0")], 1_700_000_000);
    assert_eq!(c.next_step(), before);
}
