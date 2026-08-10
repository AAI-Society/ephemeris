use ephemeris::chain::{extend, GENESIS};
use ephemeris::record::{digest, Claim, Record};
use ephemeris::tree::{leaf_hash, root_of};
use ephemeris::verify::{against_root, PriorRoot, VerifyError};

fn claim(id: &str, verdict: &str) -> Claim {
    Claim {
        action_id: id.into(),
        agent_id: "did:web:example.org:agents:ref-1".into(),
        initiating_user: "user:alice".into(),
        interception_point: "PRE_CALL_TOOL_INVOCATION".into(),
        target_resource: "/v1/charges".into(),
        canonical_snapshot_hash: "54c323d3".into(),
        path_summary_hash: None,
        policy_bundle_hash: "a7713be5".into(),
        verdict: verdict.into(),
        nonce: "n-1".into(),
    }
}

/// Build a fully self-consistent log. `tamper_at` rewrites one verdict and
/// re-links everything after it, which is exactly what an operator holding
/// the signing key can do.
fn build(n: u64, tamper_at: Option<u64>) -> Vec<Record> {
    let mut out = Vec::new();
    let mut head = GENESIS;
    for i in 0..n {
        let verdict = if Some(i) == tamper_at {
            "DENY"
        } else {
            "ALLOW"
        };
        let r = Record::new(
            claim(&format!("a-{i}"), verdict),
            i,
            head,
            1_700_000_000 + i,
        );
        head = extend(head, &r).unwrap();
        out.push(r);
    }
    out
}

fn root(records: &[Record]) -> [u8; 32] {
    let ls: Vec<[u8; 32]> = records
        .iter()
        .map(|r| leaf_hash(&digest(r).unwrap()))
        .collect();
    root_of(&ls)
}

#[test]
fn a_rewritten_and_relinked_log_is_internally_perfect() {
    // The premise of the attack, asserted so nobody mistakes the defence for
    // the chain check. Every link verifies. Nothing looks broken.
    let tampered = build(8, Some(3));
    assert!(ephemeris::chain::verify_sequence(&tampered).is_ok());
}

#[test]
fn without_a_prior_root_the_rewrite_is_accepted() {
    // Stated plainly because it is the tool's honest limitation: with no
    // externally published prior root there is nothing to contradict a
    // rewrite. This is C7.3.3, which Phase 1 does not defend.
    let tampered = build(8, Some(3));
    assert!(against_root(&tampered, None).is_ok());
}

#[test]
fn against_a_prior_root_the_rewrite_is_rejected() {
    let honest = build(8, None);
    let published = PriorRoot {
        size: 8,
        root: root(&honest),
    };

    let mut tampered = build(8, Some(3));
    tampered.extend(build(12, Some(3)).into_iter().skip(8));

    assert!(matches!(
        against_root(&tampered, Some(published)),
        Err(VerifyError::Inconsistent { .. })
    ));
}

#[test]
fn an_honest_extension_is_accepted_against_the_same_prior_root() {
    // The negative control. Without it, a verifier that rejected everything
    // would pass the test above.
    let honest8 = build(8, None);
    let published = PriorRoot {
        size: 8,
        root: root(&honest8),
    };
    let honest12 = build(12, None);
    assert!(against_root(&honest12, Some(published)).is_ok());
}

#[test]
fn a_step_count_comparison_alone_would_have_accepted_the_rewrite() {
    // C7.3.5's own warning, pinned as a test: the rewritten log has the
    // right length. Length is not evidence.
    let honest = build(8, None);
    let mut tampered = build(8, Some(3));
    tampered.extend(build(12, Some(3)).into_iter().skip(8));
    assert_eq!(honest.len(), 8);
    assert_eq!(tampered.len(), 12);
    assert_ne!(root(&honest), root(&tampered[..8]));
}
