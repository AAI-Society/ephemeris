#![cfg(feature = "bench")]

//! The benchmark's headline unit is acknowledgements per second, and an
//! acknowledgement in this crate is a claim about durability. So the harness
//! reopens the store after the timed window and checks that what it acked is
//! actually there.
//!
//! A check that cannot fail is worth nothing, so every test below makes it
//! fail on purpose: a log that lost an acked record, and a log that recovers
//! cleanly but does not verify against itself.

use ephemeris::bench::run::{claim, run, verify_durable, RunConfig, RunError};
use ephemeris::bench::TopologyKind;
use ephemeris::chain::{extend, GENESIS};
use ephemeris::record::Record;
use ephemeris::store::Store;
use std::fs::OpenOptions;

fn cfg(agents: usize, per: usize) -> RunConfig {
    RunConfig {
        topology: TopologyKind::Global,
        agents,
        claims_per_agent: per,
    }
}

/// The check has to pass on a healthy run, or it would just be an assertion
/// that the benchmark never works.
#[test]
fn a_healthy_run_verifies_against_its_own_store() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(&cfg(4, 5), dir.path()).expect("a healthy run must report");
    assert_eq!(report.claims, 20);
    assert_eq!(report.failed, 0);

    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.records().len(), report.claims);
    verify_durable(dir.path(), report.claims).expect("the recovered log must verify");
}

/// The failure this check exists for: an acknowledgement handed out for a
/// record that is not on disk. Truncating the tail of the segment makes the
/// last frame torn, so `Store::open` drops it exactly as it would after a
/// crash between the write and the sync — which is what "acked before
/// durable" looks like from the outside.
#[test]
fn an_acked_claim_that_is_not_in_the_store_is_refused_not_reported() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(&cfg(4, 5), dir.path()).unwrap();
    let acked = report.claims;

    let path = Store::open(dir.path()).unwrap().path().to_path_buf();
    let len = std::fs::metadata(&path).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(len - 16)
        .unwrap();

    let recovered = Store::open(dir.path()).unwrap().records().len();
    assert_eq!(recovered, acked - 1, "the truncation must lose one record");

    match verify_durable(dir.path(), acked) {
        Err(RunError::NotDurable {
            acked: a,
            recovered: r,
        }) => {
            assert_eq!(a, acked);
            assert_eq!(r, acked - 1);
        }
        other => panic!("a missing acked record must not be reported as a run: {other:?}"),
    }
}

/// Counting records is not enough. `Store::open` recomputes the head as it
/// reads and never compares it to what each record declares, so a log with
/// the right number of records and the wrong links recovers without
/// complaint. `verify_sequence` is what reads those fields.
#[test]
fn a_log_with_the_right_count_and_the_wrong_links_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let mut head = GENESIS;
    let mut records = Vec::new();
    for i in 0..4u64 {
        let mut r = Record::new(claim("agent-00000", i as usize), i, head, 1_700_000_000);
        head = extend(head, &r).unwrap();
        if i == 2 {
            // A head that no predecessor produced. The record is still
            // well-formed and its frame digest still matches its bytes, so
            // recovery accepts it.
            r.chain_head = "00".repeat(32);
        }
        records.push(r);
    }
    store.append_batch(&records).unwrap();

    assert_eq!(
        Store::open(dir.path()).unwrap().records().len(),
        4,
        "recovery must accept the tampered log, or this test proves nothing"
    );

    match verify_durable(dir.path(), 4) {
        Err(RunError::ChainBroken(e)) => {
            let msg = e.to_string();
            assert!(msg.contains("chain"), "unexpected chain error: {msg}");
        }
        other => panic!("a log that does not verify must not be reported: {other:?}"),
    }
}
