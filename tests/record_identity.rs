use ephemeris::record::{canonical, digest, Claim, Record};

fn claim() -> Claim {
    Claim {
        action_id: "a-1".into(),
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

fn record() -> Record {
    Record::new(claim(), 0, [0u8; 32], 1_700_000_000)
}

#[test]
fn our_canonical_form_is_rfc_8785_of_the_same_content() {
    // The meaningful version of "order independent". Serializing a Rust
    // struct is trivially ordered by declaration, so comparing two structs
    // proves nothing. This compares our output against the canonical form of
    // the *same content written with its keys shuffled* — which is what a
    // second implementation would hand us.
    let ours = canonical(&record()).unwrap();

    let shuffled = r#"{
        "eat_profile": "https://advancedaisociety.org/poc/v0.1",
        "nonce": "n-1",
        "iat": 1700000000,
        "chain_head": "0000000000000000000000000000000000000000000000000000000000000000",
        "step_index": 0,
        "claim": {
            "verdict": "ALLOW",
            "nonce": "n-1",
            "policy_bundle_hash": "a7713be5",
            "path_summary_hash": null,
            "canonical_snapshot_hash": "54c323d3",
            "target_resource": "/v1/charges",
            "interception_point": "PRE_CALL_TOOL_INVOCATION",
            "initiating_user": "user:alice",
            "agent_id": "did:web:example.org:agents:ref-1",
            "action_id": "a-1"
        }
    }"#;
    let parsed = transit::json::parse_strict(shuffled).unwrap();
    assert_eq!(ours, transit::jcs::canonicalize(&parsed).unwrap());
}

#[test]
fn every_claim_field_contributes_to_the_digest() {
    // The other half of the identity rule: differing in any one field
    // that ought to distinguish two records must change the digest.
    let base = record();
    let base_digest = digest(&base).unwrap();

    // Named rather than inlined only to satisfy `clippy::type_complexity`,
    // which the crate treats as an error. The shape is unchanged.
    type Mutator = (&'static str, Box<dyn Fn(&mut Claim)>);

    let mutators: Vec<Mutator> = vec![
        (
            "action_id",
            Box::new(|c: &mut Claim| c.action_id = "a-2".into()),
        ),
        (
            "agent_id",
            Box::new(|c: &mut Claim| c.agent_id = "did:web:other".into()),
        ),
        (
            "initiating_user",
            Box::new(|c: &mut Claim| c.initiating_user = "user:bob".into()),
        ),
        (
            "interception_point",
            Box::new(|c: &mut Claim| c.interception_point = "TASK_COMPLETION".into()),
        ),
        (
            "target_resource",
            Box::new(|c: &mut Claim| c.target_resource = "/v1/refunds".into()),
        ),
        (
            "canonical_snapshot_hash",
            Box::new(|c: &mut Claim| c.canonical_snapshot_hash = "deadbeef".into()),
        ),
        (
            "path_summary_hash",
            Box::new(|c: &mut Claim| c.path_summary_hash = Some("cafe".into())),
        ),
        (
            "policy_bundle_hash",
            Box::new(|c: &mut Claim| c.policy_bundle_hash = "feedface".into()),
        ),
        (
            "verdict",
            Box::new(|c: &mut Claim| c.verdict = "DENY".into()),
        ),
        (
            "nonce",
            Box::new(|c: &mut Claim| c.nonce = "n-other".into()),
        ),
    ];

    for (name, mutate) in mutators {
        let mut c = claim();
        mutate(&mut c);
        let r = Record::new(c, 0, [0u8; 32], 1_700_000_000);
        assert_ne!(
            digest(&r).unwrap(),
            base_digest,
            "changing {name} did not change the record digest"
        );
    }
}

#[test]
fn log_supplied_fields_also_contribute() {
    let base_digest = digest(&record()).unwrap();
    assert_ne!(
        digest(&Record::new(claim(), 1, [0u8; 32], 1_700_000_000)).unwrap(),
        base_digest
    );
    assert_ne!(
        digest(&Record::new(claim(), 0, [1u8; 32], 1_700_000_000)).unwrap(),
        base_digest
    );
    assert_ne!(
        digest(&Record::new(claim(), 0, [0u8; 32], 1_700_000_001)).unwrap(),
        base_digest
    );
}

#[test]
fn the_record_nonce_is_the_claim_nonce_and_is_never_minted() {
    // C7.1.4's challenge comes from the relying party. `Record` copies it
    // from the claim and there is no other source, so no code path can
    // manufacture one — which would produce a record that validates and
    // resists no replay at all.
    let mut c = claim();
    c.nonce = "challenge-from-the-relying-party".into();
    let r = Record::new(c, 0, [0u8; 32], 1_700_000_000);
    assert_eq!(r.nonce, "challenge-from-the-relying-party");
    assert_ne!(digest(&r).unwrap(), digest(&record()).unwrap());
}
