use ephemeris::record::{Claim, Record};
use ephemeris::store::Store;
use std::fs::OpenOptions;
use std::io::Write;

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

fn tmpdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ephemeris-test-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn records_survive_a_reopen() {
    let d = tmpdir("reopen");
    {
        let mut s = Store::open(&d).unwrap();
        for i in 0..5u64 {
            let r = Record::new(
                claim(&format!("a-{i}")),
                s.next_step(),
                s.head(),
                1_700_000_000 + i,
            );
            s.append_batch(&[r]).unwrap();
        }
        assert_eq!(s.records().len(), 5);
    }
    let s = Store::open(&d).unwrap();
    assert_eq!(s.records().len(), 5);
    assert_eq!(s.next_step(), 5);
}

#[test]
fn a_torn_write_is_truncated_not_accepted() {
    let d = tmpdir("torn");
    {
        let mut s = Store::open(&d).unwrap();
        for i in 0..3u64 {
            let r = Record::new(
                claim(&format!("a-{i}")),
                s.next_step(),
                s.head(),
                1_700_000_000 + i,
            );
            s.append_batch(&[r]).unwrap();
        }
    }
    // Simulate a process death mid-write: a length header and a few bytes.
    let mut f = OpenOptions::new()
        .append(true)
        .open(d.join("segment-0.log"))
        .unwrap();
    f.write_all(&[0, 0, 4, 0]).unwrap();
    f.write_all(b"{\"cl").unwrap();
    drop(f);

    let s = Store::open(&d).unwrap();
    assert_eq!(
        s.records().len(),
        3,
        "the torn record must not be recovered"
    );
    assert_eq!(s.next_step(), 3);
}

#[test]
fn a_corrupted_complete_record_is_refused_not_skipped() {
    // A record whose trailing digest does not match its bytes is not a torn
    // write; it is an alteration. Recovery stops rather than silently
    // dropping it, because silently dropping evidence is the failure this
    // whole tool exists to prevent.
    let d = tmpdir("corrupt");
    {
        let mut s = Store::open(&d).unwrap();
        for i in 0..3u64 {
            let r = Record::new(
                claim(&format!("a-{i}")),
                s.next_step(),
                s.head(),
                1_700_000_000 + i,
            );
            s.append_batch(&[r]).unwrap();
        }
    }
    let p = d.join("segment-0.log");
    let mut bytes = std::fs::read(&p).unwrap();
    let n = bytes.len();
    bytes[n - 40] ^= 0xff;
    std::fs::write(&p, &bytes).unwrap();

    assert!(Store::open(&d).is_err());
}

#[test]
fn head_and_next_step_advance_together() {
    let d = tmpdir("advance");
    let mut s = Store::open(&d).unwrap();
    assert_eq!(s.next_step(), 0);
    assert_eq!(s.head(), ephemeris::chain::GENESIS);
    let r = Record::new(claim("a-0"), 0, ephemeris::chain::GENESIS, 1_700_000_000);
    s.append_batch(&[r]).unwrap();
    assert_eq!(s.next_step(), 1);
    assert_ne!(s.head(), ephemeris::chain::GENESIS);
}
