use ephemeris::chain::{extend, verify_sequence, ChainError, GENESIS};
use ephemeris::record::{Claim, Record};

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

/// Build a chained run of `n` records the way the store will.
fn run(n: u64) -> Vec<Record> {
    let mut out = Vec::new();
    let mut head = GENESIS;
    for i in 0..n {
        let r = Record::new(claim(&format!("a-{i}")), i, head, 1_700_000_000 + i);
        head = extend(head, &r).unwrap();
        out.push(r);
    }
    out
}

#[test]
fn a_chain_of_records_verifies() {
    assert!(verify_sequence(&run(8)).is_ok());
}

#[test]
fn altering_a_record_breaks_the_chain() {
    let mut rs = run(8);
    rs[3].claim.verdict = "DENY".into();
    assert!(matches!(
        verify_sequence(&rs),
        Err(ChainError::HeadMismatch { at: 4, .. })
    ));
}

#[test]
fn reordering_two_records_breaks_the_chain() {
    let mut rs = run(8);
    rs.swap(3, 4);
    assert!(verify_sequence(&rs).is_err());
}

#[test]
fn a_gap_in_step_index_is_detected() {
    // C7.3.1 says a gap is an omission. The chain alone would not catch a
    // removed record if the remover also re-linked, so step_index carries
    // its own contiguity check.
    let mut rs = run(8);
    rs.remove(3);
    assert!(matches!(
        verify_sequence(&rs),
        Err(ChainError::StepGap { expected: 3, found: 4 })
    ));
}

#[test]
fn the_first_record_must_chain_from_genesis() {
    let r = Record::new(claim("a-0"), 0, [9u8; 32], 1_700_000_000);
    assert!(matches!(
        verify_sequence(&[r]),
        Err(ChainError::HeadMismatch { at: 0, .. })
    ));
}
