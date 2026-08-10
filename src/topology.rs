/// Which tree a record's leaf lands in. A newtype rather than a bare `String`
/// so a tree key cannot be confused with an agent id, which is exactly the
/// kind of casual identity that caused four of `parallax`'s five criticals.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TreeId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorPolicy {
    None,
    Joint { interval_secs: u64 },
}

/// How the log partitions its leaves.
///
/// The three implementations differ in throughput **and** in what the chain
/// reveals about who acted, and the two axes point in opposite directions.
/// A global tree leaks nothing about identity, because membership
/// distinguishes nobody; a per-agent tree's identity *is* an agent
/// identifier, so every action in it is linked to every other by
/// construction. P08 prefers per-agent for throughput; P05 objects to it for
/// exactly that reason. Measuring both axes is Phase 2's `bench`.
pub trait Topology {
    fn tree_for(&self, agent: &str) -> TreeId;
    fn anchor_policy(&self) -> AnchorPolicy;
}

/// One tree for the whole deployment. Total order; serial ceiling; the tree
/// key reveals nothing about who acted.
#[derive(Clone, Copy, Debug)]
pub struct Global;

/// One tree per agent. Parallel; no ordering across agents; **the tree key is
/// an agent identifier.**
#[derive(Clone, Copy, Debug)]
pub struct PerAgent;

/// One tree per agent, plus a joint root published every `interval_secs`,
/// giving coarse cross-agent ordering at anchor granularity.
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    pub interval_secs: u64,
}

/// Tree keys are namespaced so that no agent id can collide with the global
/// key, whatever the agent is called. The prefix is part of the identity and
/// is why `TreeId` is not simply the agent id.
const GLOBAL_KEY: &str = "tree:global";
const AGENT_PREFIX: &str = "tree:agent:";

impl Topology for Global {
    fn tree_for(&self, _agent: &str) -> TreeId {
        TreeId(GLOBAL_KEY.to_string())
    }
    fn anchor_policy(&self) -> AnchorPolicy {
        AnchorPolicy::None
    }
}

impl Topology for PerAgent {
    fn tree_for(&self, agent: &str) -> TreeId {
        TreeId(format!("{AGENT_PREFIX}{agent}"))
    }
    fn anchor_policy(&self) -> AnchorPolicy {
        AnchorPolicy::None
    }
}

impl Topology for Joint {
    fn tree_for(&self, agent: &str) -> TreeId {
        TreeId(format!("{AGENT_PREFIX}{agent}"))
    }
    fn anchor_policy(&self) -> AnchorPolicy {
        AnchorPolicy::Joint {
            interval_secs: self.interval_secs,
        }
    }
}

/// A set of trees, with a topology deciding which one each leaf lands in.
///
/// This is the trait's consumer, and it exists so the equivalence property
/// can be stated: a topology decides *where* a leaf goes, never *what* the
/// record says. Phase 2's benchmark depends on that being true, because
/// otherwise it is measuring three different systems rather than three
/// placements of one.
pub struct Forest<T: Topology> {
    topology: T,
    trees: std::collections::BTreeMap<TreeId, crate::tree::Tree>,
    leaf_count: u64,
}

impl<T: Topology> Forest<T> {
    pub fn new(topology: T) -> Self {
        Self {
            topology,
            trees: std::collections::BTreeMap::new(),
            leaf_count: 0,
        }
    }

    pub fn insert(&mut self, agent: &str, leaf: [u8; 32]) {
        let id = self.topology.tree_for(agent);
        self.trees.entry(id).or_default().append(leaf);
        self.leaf_count += 1;
    }

    pub fn tree_count(&self) -> usize {
        self.trees.len()
    }

    pub fn leaf_count(&self) -> u64 {
        self.leaf_count
    }

    /// Every tree's root, keyed by tree. `BTreeMap` so the order is the tree
    /// key's order and never insertion order — two forests holding the same
    /// trees must compare equal regardless of the sequence that built them.
    pub fn roots(&self) -> std::collections::BTreeMap<TreeId, [u8; 32]> {
        self.trees
            .iter()
            .map(|(k, v)| (k.clone(), v.root()))
            .collect()
    }

    pub fn anchor_policy(&self) -> AnchorPolicy {
        self.topology.anchor_policy()
    }
}
