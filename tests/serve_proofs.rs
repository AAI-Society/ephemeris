#![cfg(feature = "serve")]

mod harness;
use harness::{get, post, TestServer};

fn seed(srv: &TestServer, n: usize) {
    for i in 0..n {
        let body = format!(
            r#"{{"action_id":"a-{i}","agent_id":"did:web:x","initiating_user":"u",
            "interception_point":"PRE_CALL_TOOL_INVOCATION","target_resource":"/x",
            "canonical_snapshot_hash":"aa","policy_bundle_hash":"bb","verdict":"ALLOW",
            "nonce":"n-{i}"}}"#
        );
        let (s, b) = post(srv.path(), "/claims?now=1700000000", &body);
        assert_eq!(s, 200, "seeding failed: {b}");
    }
}

#[test]
fn the_root_route_says_what_it_does_not_defend() {
    // The same sentence the CLI prints, from the same renderer. If this ever
    // stops matching, two formatters exist and one of them will drift on
    // exactly the text this project least wants reworded.
    let srv = TestServer::start();
    seed(&srv, 1);
    let (status, body) = get(srv.path(), "/root");
    assert_eq!(status, 200);
    assert!(
        body.contains("C7.3.3"),
        "root did not print its limitation: {body}"
    );
    assert!(body.contains("SOFTWARE"));
}

#[test]
fn an_inclusion_proof_served_over_the_socket_verifies() {
    let srv = TestServer::start();
    seed(&srv, 8);
    let (status, body) = get(srv.path(), "/proof/inclusion?leaf=3&size=8");
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(
        v["path"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "empty path: {body}"
    );
}

#[test]
fn a_consistency_proof_served_over_the_socket_verifies() {
    let srv = TestServer::start();
    seed(&srv, 8);
    let (status, body) = get(srv.path(), "/proof/consistency?from=4&to=8");
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(v["proof"].is_array(), "no proof array: {body}");
}

#[test]
fn out_of_range_proof_requests_are_refused_not_fabricated() {
    let srv = TestServer::start();
    seed(&srv, 4);
    for target in [
        "/proof/inclusion?leaf=99&size=4",
        "/proof/inclusion?size=4",
        "/proof/consistency?from=9&to=4",
        "/proof/consistency?from=1",
        // These two specifically exceed the durable log's actual size (4),
        // exercising the `size > n` / `to > n` guards in routes.rs directly
        // — the four cases above are each caught by a different guard
        // (inclusion_proof's own bound, a missing param, or `from > to`)
        // and never reach these two at all.
        "/proof/inclusion?leaf=0&size=99",
        "/proof/consistency?from=1&to=99",
    ] {
        let (status, body) = get(srv.path(), target);
        assert_ne!(
            status, 200,
            "{target} returned a proof it cannot have: {body}"
        );
    }
    // The two size-exceeding cases must be refused specifically as "no such
    // range", not merely as some other non-200 — pin the status precisely so
    // this discriminates the `size > n` / `to > n` guards, not just any error
    // path.
    let (status, body) = get(srv.path(), "/proof/inclusion?leaf=0&size=99");
    assert_eq!(status, 404, "{body}");
    let (status, body) = get(srv.path(), "/proof/consistency?from=1&to=99");
    assert_eq!(status, 404, "{body}");
}

/// The component's whole reason to exist, checked over the transport a real
/// auditor would use rather than only in process.
///
/// `tests/a9_rewrite.rs` is the reference for this construction and it is
/// followed here, not re-derived: rewrite one record's verdict, relink every
/// record after it, and confirm the result is internally perfect but still
/// rejected against a root published before the rewrite.
///
/// The socket has no route that can produce the tampered half. It only ever
/// appends — there is no way to ask `POST /claims` to rewrite step 3 — so
/// producing that state has to happen off the socket, on disk, with the
/// crate's own `Store`/`Record`/`extend` primitives. What stays on the wire
/// is everything that matters for the property under test: the honest half
/// (seeding and publishing a root) goes through the real `POST /claims` and
/// `GET /root`, and the tampered log is served from a second, independent
/// listener through the real `GET /proof/consistency` route, checked with
/// `ephemeris::tree::verify_consistency` — the same verifier the CLI uses —
/// so a serialization bug in the route would surface here, not just a logic
/// bug in the verifier.
#[test]
fn a_rewritten_and_relinked_log_still_fails_a_consistency_check_over_the_socket() {
    use ephemeris::chain::{extend, verify_sequence};
    use ephemeris::record::{Claim, Record};
    use ephemeris::store::Store;
    use ephemeris::tree::verify_consistency;

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
            nonce: id.into(),
        }
    }

    fn claim_body(id: &str, verdict: &str) -> String {
        format!(
            r#"{{"action_id":"{id}","agent_id":"did:web:example.org:agents:ref-1",
            "initiating_user":"user:alice","interception_point":"PRE_CALL_TOOL_INVOCATION",
            "target_resource":"/v1/charges","canonical_snapshot_hash":"54c323d3",
            "policy_bundle_hash":"a7713be5","verdict":"{verdict}","nonce":"{id}"}}"#
        )
    }

    fn hex32(s: &str) -> [u8; 32] {
        hex::decode(s).expect("hex").try_into().expect("32 bytes")
    }

    /// Append `from..to` fresh honest records onto `records`, chaining from
    /// `head`.
    fn extend_with(
        records: &mut Vec<Record>,
        mut head: [u8; 32],
        from: u64,
        to: u64,
        prefix: &str,
    ) {
        for i in from..to {
            let r = Record::new(
                claim(&format!("{prefix}{i}"), "ALLOW"),
                i,
                head,
                1_700_000_000 + i,
            );
            head = extend(head, &r).expect("extend");
            records.push(r);
        }
    }

    fn consistency_from_socket(
        sock: &std::path::Path,
    ) -> (serde_json::Value, [u8; 32], Vec<[u8; 32]>) {
        let (status, body) = get(sock, "/proof/consistency?from=8&to=12");
        assert_eq!(status, 200, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        let new_root = hex32(v["new_root"].as_str().expect("new_root"));
        let proof: Vec<[u8; 32]> = v["proof"]
            .as_array()
            .expect("proof array")
            .iter()
            .map(|e| hex32(e.as_str().expect("hex")))
            .collect();
        (v, new_root, proof)
    }

    // --- Monday: seed 8 honest claims through the real socket, then publish
    // a root through the real GET /root route. ---
    let monday = TestServer::start();
    for i in 0..8u64 {
        let (s, b) = post(
            monday.path(),
            &format!("/claims?now={}", 1_700_000_000 + i),
            &claim_body(&format!("a-{i}"), "ALLOW"),
        );
        assert_eq!(s, 200, "seeding failed: {b}");
    }
    let (status, body) = get(monday.path(), "/root");
    assert_eq!(status, 200, "{body}");
    let published: serde_json::Value = serde_json::from_str(&body).expect("json");
    let published_size = published["size"].as_u64().expect("size");
    let published_root = hex32(published["root"].as_str().expect("root"));
    assert_eq!(published_size, 8);

    // Read back exactly what Monday's log holds. The tampered log below is
    // built by mutating these bytes, not a parallel reconstruction of them —
    // a rewrite of anything other than what was actually committed and
    // published would be a different, weaker demonstration.
    let honest: Vec<Record> = Store::open(monday.store_dir())
        .expect("reopen")
        .records()
        .to_vec();
    assert_eq!(honest.len(), 8);

    // --- Tuesday: record 3's verdict is rewritten and the chain relinked —
    // exactly tests/a9_rewrite.rs's construction, applied to a log a real
    // socket actually committed. Extended to size 12 so the request below is
    // a genuine "prove you're still an extension of what I saw at 8" ask. ---
    let mut tampered = honest.clone();
    // `Record::chain_head` stores the head *going into* that record (what
    // `extend` was called with to produce it), not the head coming out of
    // it — so the head record 3 needs is the one already recorded on record
    // 3 itself, not derived from record 2.
    let head_before_3 = hex32(&honest[3].chain_head);
    tampered[3] = Record::new(claim("a-3", "DENY"), 3, head_before_3, tampered[3].iat);
    let mut head = extend(head_before_3, &tampered[3]).expect("extend");
    for i in 4..8u64 {
        let r = Record::new(
            claim(&format!("a-{i}"), "ALLOW"),
            i,
            head,
            tampered[i as usize].iat,
        );
        head = extend(head, &r).expect("extend");
        tampered[i as usize] = r;
    }
    extend_with(&mut tampered, head, 8, 12, "a-");
    assert_eq!(tampered.len(), 12);

    // The premise of the attack, asserted so nobody mistakes the defence
    // below for a chain check that would have caught this some other way.
    assert_ne!(
        tampered[3].claim.verdict, honest[3].claim.verdict,
        "the tamper changed nothing"
    );
    assert_ne!(
        tampered[4].chain_head, honest[4].chain_head,
        "relinking should have changed the head every later record commits to"
    );
    assert!(
        verify_sequence(&tampered).is_ok(),
        "a rewritten-and-relinked log must still be internally perfect, or this isn't attack A9"
    );

    // The socket cannot produce this state, so it is written directly, with
    // the crate's own Store, then served from a second, independent
    // listener.
    let tuesday_dir = tempfile::tempdir().expect("tempdir");
    {
        let mut store = Store::open(tuesday_dir.path()).expect("store");
        store.append_batch(&tampered).expect("append");
    }
    let tuesday = TestServer::start_at(tuesday_dir.path(), &tuesday_dir.path().join("eph.sock"));

    // Internally perfect over the socket too: the tampered store's own
    // GET /root succeeds. Nothing here is what catches the rewrite — that is
    // the whole point of A9.
    let (status, body) = get(tuesday.path(), "/root");
    assert_eq!(
        status, 200,
        "a rewritten log should still look perfect: {body}"
    );

    // The check that matters: a served consistency proof from Monday's
    // published size to Tuesday's tampered size 12 must not verify against
    // the root Monday actually published.
    let (v, new_root, proof) = consistency_from_socket(tuesday.path());
    let served_old_root = hex32(v["old_root"].as_str().expect("old_root"));
    assert_ne!(
        served_old_root, published_root,
        "the tampered log's own prefix at size 8 must differ from what Monday published, \
         or this test is not exercising A9 at all"
    );
    assert!(
        !verify_consistency(published_size, published_root, 12, new_root, &proof),
        "a rewritten and relinked log verified against a root published before the rewrite"
    );

    // Negative control: an honest continuation of the SAME published prefix
    // must verify. Without this, a verifier that rejected every consistency
    // check would pass the assertion above for the wrong reason — the same
    // shape as tests/a9_rewrite.rs's own
    // `an_honest_extension_is_accepted_against_the_same_prior_root`.
    let mut honest_extended = honest.clone();
    let head = hex32(&honest_extended[7].chain_head);
    extend_with(&mut honest_extended, head, 8, 12, "b-");
    let wednesday_dir = tempfile::tempdir().expect("tempdir");
    {
        let mut store = Store::open(wednesday_dir.path()).expect("store");
        store.append_batch(&honest_extended).expect("append");
    }
    let wednesday =
        TestServer::start_at(wednesday_dir.path(), &wednesday_dir.path().join("eph.sock"));
    let (_, new_root, proof) = consistency_from_socket(wednesday.path());
    assert!(
        verify_consistency(published_size, published_root, 12, new_root, &proof),
        "an honest extension of the published root was refused"
    );
}
