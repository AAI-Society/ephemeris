use ephemeris::tree::{
    consistency_proof, inclusion_proof, leaf_hash, root_of, verify_consistency, verify_inclusion,
};

fn leaves(n: usize) -> Vec<[u8; 32]> {
    (0..n).map(|i| leaf_hash(&[i as u8])).collect()
}

#[test]
fn every_leaf_of_every_tree_up_to_33_proves_inclusion() {
    for n in 1..=33usize {
        let ls = leaves(n);
        let root = root_of(&ls);
        for m in 0..n {
            let p = inclusion_proof(&ls, m).expect("proof for an in-range leaf");
            assert!(
                verify_inclusion(ls[m], m as u64, n as u64, &p, root),
                "leaf {m} of {n} failed to verify"
            );
        }
    }
}

#[test]
fn an_inclusion_proof_does_not_verify_against_a_different_leaf() {
    let ls = leaves(8);
    let root = root_of(&ls);
    let p = inclusion_proof(&ls, 3).unwrap();
    assert!(!verify_inclusion(ls[4], 3, 8, &p, root));
}

#[test]
fn an_out_of_range_leaf_has_no_proof() {
    assert!(inclusion_proof(&leaves(8), 8).is_none());
}

#[test]
fn proof_size_is_logarithmic_not_linear() {
    // C7.3.4's auditor evidence asks for exactly this check.
    let ls = leaves(1024);
    let p = inclusion_proof(&ls, 500).unwrap();
    assert!(
        p.len() <= 10,
        "proof of {} elements for 1024 leaves",
        p.len()
    );
}

#[test]
fn every_prefix_is_consistent_with_every_extension() {
    for n in 1..=33usize {
        let ls = leaves(n);
        let new_root = root_of(&ls);
        for m in 1..=n {
            let old_root = root_of(&ls[..m]);
            let p = consistency_proof(&ls, m).expect("proof for an in-range prefix");
            assert!(
                verify_consistency(m as u64, old_root, n as u64, new_root, &p),
                "prefix {m} of {n} failed to verify"
            );
        }
    }
}

#[test]
fn a_rewritten_history_fails_consistency() {
    // The whole point. Take an 8-leaf log, publish its root, then rewrite
    // leaf 2 and extend to 12. The new tree is internally perfect and
    // inconsistent with the published root.
    let original = leaves(8);
    let published = root_of(&original);

    let mut rewritten = leaves(12);
    rewritten[2] = leaf_hash(b"tampered");
    let new_root = root_of(&rewritten);

    let p = consistency_proof(&rewritten, 8).unwrap();
    assert!(!verify_consistency(8, published, 12, new_root, &p));
}
