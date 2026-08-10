use sha2::{Digest, Sha256};

/// RFC 6962 §2.1 domain separation. A leaf and an interior node must hash
/// differently, or a leaf can be presented as a node and the tree's shape
/// rewritten.
const LEAF_PREFIX: u8 = 0x00;
const NODE_PREFIX: u8 = 0x01;

/// MTH({}) = SHA-256() over the empty string, per RFC 6962 §2.1.
pub const EMPTY_ROOT: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

pub fn leaf_hash(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([LEAF_PREFIX]);
    h.update(data);
    h.finalize().into()
}

pub fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([NODE_PREFIX]);
    h.update(left);
    h.update(right);
    h.finalize().into()
}

/// The reference recursive Merkle tree head, written straight from RFC 6962
/// §2.1. Used by `Tree` for proofs and by the tests as the oracle the
/// incremental path must agree with.
pub fn root_of(leaves: &[[u8; 32]]) -> [u8; 32] {
    match leaves.len() {
        0 => EMPTY_ROOT,
        1 => leaves[0],
        n => {
            let k = largest_power_of_two_below(n);
            node_hash(&root_of(&leaves[..k]), &root_of(&leaves[k..]))
        }
    }
}

/// The largest power of two strictly less than `n`, for `n > 1`. RFC 6962
/// splits every interior node at exactly this point, so an off-by-one here
/// produces a tree that is internally consistent and incompatible with every
/// other implementation.
pub(crate) fn largest_power_of_two_below(n: usize) -> usize {
    debug_assert!(n > 1);
    1usize << (usize::BITS - 1 - (n - 1).leading_zeros())
}

/// An append-only tree that keeps its right fringe, so appending is O(log n)
/// and the head is available without rescanning.
///
/// The full leaf vector is kept alongside it because inclusion and consistency
/// proofs need arbitrary subtrees, not only the fringe. That makes proof
/// generation O(n); whether proof serving becomes the bottleneck under audit
/// load is P08's open question 5 and is measured in Phase 2, not guessed at
/// here.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// Complete subtrees, largest first. `fringe[i]` is the root of a complete
    /// subtree, and the sizes are the set bits of `size` from high to low.
    fringe: Vec<[u8; 32]>,
    leaves: Vec<[u8; 32]>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(&self) -> u64 {
        self.leaves.len() as u64
    }

    pub fn leaves(&self) -> &[[u8; 32]] {
        &self.leaves
    }

    pub fn append(&mut self, leaf: [u8; 32]) {
        let mut carry = leaf;
        let mut n = self.leaves.len();
        // Every set low bit of the current size is a complete subtree of the
        // same height as `carry`, so it merges. The loop stops at the first
        // clear bit, which is where `carry` comes to rest.
        while n & 1 == 1 {
            if let Some(left) = self.fringe.pop() {
                carry = node_hash(&left, &carry);
            }
            n >>= 1;
        }
        self.fringe.push(carry);
        self.leaves.push(leaf);
    }

    /// Fold the fringe right to left: the smallest subtree is the rightmost,
    /// and each larger one sits to its left.
    pub fn root(&self) -> [u8; 32] {
        let mut acc: Option<[u8; 32]> = None;
        for h in self.fringe.iter().rev() {
            acc = Some(match acc {
                None => *h,
                Some(right) => node_hash(h, &right),
            });
        }
        acc.unwrap_or(EMPTY_ROOT)
    }
}

/// RFC 6962 §2.1.1 PATH(m, D[n]).
pub fn inclusion_proof(leaves: &[[u8; 32]], m: usize) -> Option<Vec<[u8; 32]>> {
    if m >= leaves.len() {
        return None;
    }
    Some(path(leaves, m))
}

fn path(leaves: &[[u8; 32]], m: usize) -> Vec<[u8; 32]> {
    let n = leaves.len();
    if n == 1 {
        return Vec::new();
    }
    let k = largest_power_of_two_below(n);
    if m < k {
        let mut p = path(&leaves[..k], m);
        p.push(root_of(&leaves[k..]));
        p
    } else {
        let mut p = path(&leaves[k..], m - k);
        p.push(root_of(&leaves[..k]));
        p
    }
}

/// Recompute the root from a leaf and its path, and compare. RFC 6962 §2.1.1.
pub fn verify_inclusion(
    leaf: [u8; 32],
    m: u64,
    size: u64,
    proof: &[[u8; 32]],
    root: [u8; 32],
) -> bool {
    if m >= size || size == 0 {
        return false;
    }
    let mut hash = leaf;
    let mut index = m;
    let mut last = size - 1;
    let mut it = proof.iter();
    while last > 0 {
        let Some(sibling) = it.next() else {
            return false;
        };
        if index % 2 == 1 || index == last {
            hash = node_hash(sibling, &hash);
            // Walk up past any right-edge shortcut.
            while index.is_multiple_of(2) && index > 0 {
                index /= 2;
                last /= 2;
            }
        } else {
            hash = node_hash(&hash, sibling);
        }
        index /= 2;
        last /= 2;
    }
    it.next().is_none() && hash == root
}

/// RFC 6962 §2.1.2 PROOF(m, D[n]).
pub fn consistency_proof(leaves: &[[u8; 32]], m: usize) -> Option<Vec<[u8; 32]>> {
    if m == 0 || m > leaves.len() {
        return None;
    }
    Some(subproof(leaves, m, true))
}

fn subproof(leaves: &[[u8; 32]], m: usize, b: bool) -> Vec<[u8; 32]> {
    let n = leaves.len();
    if m == n {
        return if b { Vec::new() } else { vec![root_of(leaves)] };
    }
    let k = largest_power_of_two_below(n);
    if m <= k {
        let mut p = subproof(&leaves[..k], m, b);
        p.push(root_of(&leaves[k..]));
        p
    } else {
        let mut p = subproof(&leaves[k..], m - k, false);
        p.push(root_of(&leaves[..k]));
        p
    }
}

/// Verify that the tree of `new_size` leaves rooted at `new_root` is an
/// append-only extension of the tree of `old_size` leaves rooted at
/// `old_root`.
///
/// **This is the function that stops attack A9.** A rewritten-and-re-signed
/// log is internally flawless and replays perfectly; the only thing that
/// contradicts it is a root published before the rewrite, compared here.
/// C7.3.5 exists because this specification's own reference implementation
/// compared record counts instead, and accepted a rewritten history.
pub fn verify_consistency(
    old_size: u64,
    old_root: [u8; 32],
    new_size: u64,
    new_root: [u8; 32],
    proof: &[[u8; 32]],
) -> bool {
    if old_size == 0 || old_size > new_size {
        return false;
    }
    if old_size == new_size {
        return proof.is_empty() && old_root == new_root;
    }

    let mut node = old_size - 1;
    let mut last = new_size - 1;
    while node % 2 == 1 {
        node /= 2;
        last /= 2;
    }

    let mut it = proof.iter();
    // A proof for a prefix that is itself a complete subtree omits that
    // subtree's root, so it is seeded from `old_root`.
    let (mut fr, mut sr) = if node > 0 {
        let Some(first) = it.next() else {
            return false;
        };
        (*first, *first)
    } else {
        (old_root, old_root)
    };

    while node > 0 {
        if node % 2 == 1 {
            let Some(sibling) = it.next() else {
                return false;
            };
            fr = node_hash(sibling, &fr);
            sr = node_hash(sibling, &sr);
        } else if node < last {
            let Some(sibling) = it.next() else {
                return false;
            };
            sr = node_hash(&sr, sibling);
        }
        node /= 2;
        last /= 2;
    }

    while last > 0 {
        let Some(sibling) = it.next() else {
            return false;
        };
        sr = node_hash(&sr, sibling);
        last /= 2;
    }

    it.next().is_none() && fr == old_root && sr == new_root
}
