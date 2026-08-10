use ephemeris::tree::{leaf_hash, node_hash, root_of, Tree, EMPTY_ROOT};

fn leaves(n: usize) -> Vec<[u8; 32]> {
    (0..n).map(|i| leaf_hash(&[i as u8])).collect()
}

#[test]
fn an_empty_tree_has_the_rfc_6962_empty_root() {
    // RFC 6962 §2.1: MTH({}) = SHA-256().
    use sha2::{Digest, Sha256};
    let want: [u8; 32] = Sha256::digest(b"").into();
    assert_eq!(EMPTY_ROOT, want);
    assert_eq!(Tree::new().root(), want);
}

#[test]
fn a_single_leaf_tree_roots_to_that_leaf() {
    let mut t = Tree::new();
    let l = leaf_hash(b"x");
    t.append(l);
    assert_eq!(t.root(), l);
}

#[test]
fn leaf_and_node_prefixes_differ() {
    // RFC 6962 §2.1 prefixes leaves with 0x00 and nodes with 0x01 precisely
    // so a leaf cannot be presented as an interior node. Without the
    // distinction a second-preimage attack rewrites the tree's shape.
    let a = leaf_hash(b"");
    let b = node_hash(&[0u8; 32], &[0u8; 32]);
    assert_ne!(a, b);
}

#[test]
fn incremental_append_agrees_with_recomputation_at_every_size() {
    // The right-fringe incremental root must equal the reference recursive
    // root for every size, not merely for powers of two — the sizes where a
    // fringe bug hides are the ones that are not.
    let ls = leaves(33);
    let mut t = Tree::new();
    for (i, l) in ls.iter().enumerate() {
        t.append(*l);
        assert_eq!(t.size(), (i + 1) as u64);
        assert_eq!(t.root(), root_of(&ls[..=i]), "root diverged at size {}", i + 1);
    }
}

#[test]
fn a_two_leaf_root_is_the_node_hash_of_its_leaves() {
    let ls = leaves(2);
    assert_eq!(root_of(&ls), node_hash(&ls[0], &ls[1]));
}
