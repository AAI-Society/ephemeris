//! Attack A9, run end to end.
//!
//! An operator holding the signing key alters one past verdict and recomputes
//! every link after it. The result is a log with no internal defect of any
//! kind. Run it and watch the internal checks pass.
//!
//!     cargo run --example rewrite

use ephemeris::chain::{extend, verify_sequence, GENESIS};
use ephemeris::record::{digest, Claim, Record};
use ephemeris::sign::{Signer, SoftwareSigner};
use ephemeris::tree::{leaf_hash, root_of};
use ephemeris::verify::{against_root, PriorRoot};

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

/// Build a fully self-consistent log of `n` records, optionally rewriting the
/// verdict at `tamper_at` and relinking everything after it.
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
        head = extend(head, &r).expect("canonicalization of a record we just built");
        out.push(r);
    }
    out
}

fn root(records: &[Record]) -> [u8; 32] {
    let ls: Vec<[u8; 32]> = records
        .iter()
        .map(|r| leaf_hash(&digest(r).expect("canonicalization of a record we just built")))
        .collect();
    root_of(&ls)
}

fn main() {
    // Monday. Eight actions are logged honestly, and the operator publishes
    // the root somewhere it cannot later reach — a transparency log, a
    // counterparty, a printout in a drawer.
    let honest = build(8, None);
    let published = PriorRoot {
        size: 8,
        root: root(&honest),
    };
    println!("Monday — root published at size 8:");
    println!("  {}\n", hex::encode(published.root));

    // Tuesday. Action 3 was a DENY that should have been an ALLOW, or the
    // reverse; it does not matter which. The operator rewrites it and
    // recomputes every chain link after it, then carries on to twelve.
    let mut rewritten = build(8, Some(3));
    rewritten.extend(build(12, Some(3)).into_iter().skip(8));

    println!("Tuesday — record 3's verdict is rewritten and the chain relinked.\n");

    // Every internal check passes. This is the point of the demonstration.
    println!(
        "  chain verifies internally:      {}",
        verify_sequence(&rewritten).is_ok()
    );
    println!(
        "  step indices contiguous:        {}",
        rewritten
            .iter()
            .enumerate()
            .all(|(i, r)| r.step_index == i as u64)
    );
    println!(
        "  accepted with no prior root:    {}",
        against_root(&rewritten, None).is_ok()
    );

    let signer = SoftwareSigner::generate([7u8; 32]);
    let now = root(&rewritten);
    let signed = signer.sign_root(&now, 12);
    println!("  root signs cleanly:             true");
    println!(
        "  signature:                      {}…\n",
        &signed.signature()[..32]
    );

    println!("Nothing is broken. There is no failing check to notice, no torn");
    println!("record, no gap in the sequence. A verifier handed this log alone");
    println!("has nothing to object to, and it is a different log than the one");
    println!("those eight actions produced.\n");

    // The only thing that contradicts it is Monday's root.
    match against_root(&rewritten, Some(published)) {
        Ok(_) => println!("REGRESSION: the rewrite was accepted against the published root."),
        Err(e) => {
            println!("Against Monday's published root:\n");
            println!("  REFUSED — {e}\n");
        }
    }

    println!("The defence is not a better checksum. It is that somebody else");
    println!("was holding a copy before the rewrite happened.");
}
