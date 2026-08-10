//! The measurement harness. Behind the `bench` feature, because it pulls
//! `occultation` and the BLS12-381 stack that comes with it.
//!
//! Nothing in here is on the write path. `CommitQueue`, which the harness
//! drives, is deliberately *not* feature-gated: measuring something other than
//! what ships would defeat the point.

pub mod host;
pub mod linkability;
pub mod run;

use crate::topology::{Global, Joint, PerAgent, Topology, TreeId};

/// The three topologies, as a value so the matrix can iterate them.
///
/// `Topology` is a trait with three unit implementations; this enum dispatches
/// to those same implementations rather than restating their rules, so a
/// change to how a topology assigns trees cannot silently fail to reach the
/// benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyKind {
    Global,
    PerAgent,
    Joint,
}

impl TopologyKind {
    pub const ALL: [TopologyKind; 3] = [
        TopologyKind::Global,
        TopologyKind::PerAgent,
        TopologyKind::Joint,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            TopologyKind::Global => "global",
            TopologyKind::PerAgent => "per-agent",
            TopologyKind::Joint => "joint",
        }
    }

    pub fn tree_for(&self, agent: &str) -> TreeId {
        match self {
            TopologyKind::Global => Global.tree_for(agent),
            TopologyKind::PerAgent => PerAgent.tree_for(agent),
            // The interval does not affect partitioning, only anchoring.
            TopologyKind::Joint => Joint { interval_secs: 60 }.tree_for(agent),
        }
    }
}
