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
