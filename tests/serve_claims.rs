#![cfg(feature = "serve")]

mod harness;
use harness::{post, TestServer};

fn claim_json(id: &str) -> String {
    format!(
        r#"{{"action_id":"{id}","agent_id":"did:web:x","initiating_user":"u",
        "interception_point":"PRE_CALL_TOOL_INVOCATION","target_resource":"/x",
        "canonical_snapshot_hash":"aa","policy_bundle_hash":"bb","verdict":"ALLOW",
        "nonce":"n-{id}"}}"#
    )
}

#[test]
fn a_claim_is_acked_and_is_durable_on_disk() {
    let srv = TestServer::start();
    let (status, body) = post(srv.path(), "/claims?now=1700000000", &claim_json("a-0"));
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("step_index"), "no ack in body: {body}");

    // Durability asserted from disk, not from the response.
    let store = ephemeris::store::Store::open(srv.store_dir()).unwrap();
    assert_eq!(store.records().len(), 1);
    ephemeris::chain::verify_sequence(store.records()).expect("chain must verify");
}

#[test]
fn a_refused_claim_yields_no_ack() {
    // The store is broken before the request. Fail-closed must hold across the
    // socket exactly as it does in process.
    let srv = TestServer::start_with_broken_store();
    let (status, body) = post(srv.path(), "/claims?now=1700000000", &claim_json("a-0"));
    assert_ne!(status, 200, "a claim was acked without being durable");
    assert!(
        !body.contains("step_index"),
        "an ack leaked on the failure path: {body}"
    );
}

#[test]
fn a_claim_with_an_unknown_field_is_refused() {
    let srv = TestServer::start();
    let bad = r#"{"action_id":"a","agent_id":"x","initiating_user":"u","interception_point":"T",
        "target_resource":"/x","canonical_snapshot_hash":"aa","policy_bundle_hash":"bb",
        "verdict":"ALLOW","nonce":"n","typo_field":1}"#;
    let (status, _) = post(srv.path(), "/claims?now=1700000000", bad);
    assert_eq!(status, 400);
}

#[test]
fn a_claim_without_now_is_refused_rather_than_timestamped_here() {
    // Time is injected, never read from the system clock. A claim arriving
    // without one must not be given the server's idea of now.
    let srv = TestServer::start();
    let (status, _) = post(srv.path(), "/claims", &claim_json("a-0"));
    assert_eq!(status, 400);
}

#[test]
fn a_claim_without_path_summary_hash_is_accepted_and_recorded_as_null() {
    // The transit-guard fork resolved to shipping the field unfilled. This is
    // the assertion that ephemeris's side of that resolution holds over the
    // socket, not only through the CLI.
    let srv = TestServer::start();
    let (status, _) = post(srv.path(), "/claims?now=1700000000", &claim_json("a-0"));
    assert_eq!(status, 200);
    let store = ephemeris::store::Store::open(srv.store_dir()).unwrap();
    assert!(store.records()[0].claim.path_summary_hash.is_none());
}
