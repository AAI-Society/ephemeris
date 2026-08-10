use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ephemeris")
}

#[test]
fn append_then_root_reports_a_signed_root() {
    let d = tempfile::tempdir().unwrap();
    let claim = r#"{"action_id":"a-0","agent_id":"did:web:x","initiating_user":"user:alice","interception_point":"PRE_CALL_TOOL_INVOCATION","target_resource":"/v1/charges","canonical_snapshot_hash":"54c323d3","policy_bundle_hash":"a7713be5","verdict":"ALLOW","nonce":"n-0"}"#;
    let f = d.path().join("claim.json");
    std::fs::write(&f, claim).unwrap();

    let out = Command::new(bin())
        .args([
            "append",
            "--dir",
            d.path().to_str().unwrap(),
            "--claim",
            f.to_str().unwrap(),
            "--now",
            "1700000000",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = Command::new(bin())
        .args(["root", "--dir", d.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("C7.3.3"),
        "the root must print what it does not defend"
    );
    assert!(text.contains("size 1"));
}

#[test]
fn a_claim_with_an_unknown_field_is_refused_with_exit_2() {
    let d = tempfile::tempdir().unwrap();
    let claim = r#"{"action_id":"a-0","agent_id":"did:web:x","initiating_user":"u","interception_point":"TASK_COMPLETION","target_resource":"/x","canonical_snapshot_hash":"aa","policy_bundle_hash":"bb","verdict":"ALLOW","nonce":"n-0","typo_field":1}"#;
    let f = d.path().join("claim.json");
    std::fs::write(&f, claim).unwrap();
    let out = Command::new(bin())
        .args([
            "append",
            "--dir",
            d.path().to_str().unwrap(),
            "--claim",
            f.to_str().unwrap(),
            "--now",
            "1700000000",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn verify_exits_1_when_the_log_contradicts_a_prior_root() {
    let d = tempfile::tempdir().unwrap();
    let claim = r#"{"action_id":"a-0","agent_id":"did:web:x","initiating_user":"u","interception_point":"TASK_COMPLETION","target_resource":"/x","canonical_snapshot_hash":"aa","policy_bundle_hash":"bb","verdict":"ALLOW","nonce":"n-0"}"#;
    let f = d.path().join("claim.json");
    std::fs::write(&f, claim).unwrap();
    Command::new(bin())
        .args([
            "append",
            "--dir",
            d.path().to_str().unwrap(),
            "--claim",
            f.to_str().unwrap(),
            "--now",
            "1700000000",
        ])
        .output()
        .unwrap();

    let out = Command::new(bin())
        .args([
            "verify",
            "--dir",
            d.path().to_str().unwrap(),
            "--prior-size",
            "1",
            "--prior-root",
            &"aa".repeat(32),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
